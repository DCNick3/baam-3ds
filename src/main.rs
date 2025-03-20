#![feature(allocator_api)]
#![feature(unsafe_cell_access)]

mod api;
mod camera;
mod flow;
mod qr;
mod settings_storage;
mod spmc_buffer;
mod ui;

use crate::ui::{UiCommand, UiHandle};
use ctru::prelude::*;
use ctru::services::cam::ViewSize;
use ctru::services::gfx::{Flush, Screen, Swap};
use ctru::services::gspgpu::FramebufferFormat;
use ctru::services::svc::handle::BorrowedThread;
use ctru::services::y2r;

fn init_tracing(soc: &mut Soc) {
    if soc.redirect_to_3dslink(false, true).is_ok() {
        tracing_subscriber::fmt::Subscriber::builder()
            .with_writer(std::io::stderr)
            .init();
    }
}

// 320x240px RGB565
// ffmpeg -vcodec png -i top_check.png -vf "transpose=1" -vcodec rawvideo -f rawvideo -pix_fmt rgb565 -y top_check.rgb
static TOP_CHECK: &[u8] = include_bytes!("../res/top_check.rgb");

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum TopMode {
    Viewfinder,
    Checkmark,
}

fn main() {
    let apt = Apt::new().unwrap();
    let mut hid = Hid::new().unwrap();
    let gfx = Gfx::new().unwrap();

    // Need sockets for connections to baam servers & (optional) 3dslink
    let mut soc = Soc::new().expect("Couldn't obtain SOC controller");

    init_tracing(&mut soc);

    let mut top_screen = gfx.top_screen.borrow_mut();

    top_screen.set_double_buffering(true);
    top_screen.set_framebuffer_format(FramebufferFormat::Rgb565);

    // Start a console on the bottom screen.
    // The most recently initialized console will be active by default.
    let bottom_screen = Console::new(gfx.bottom_screen.borrow_mut());

    let main_thread_prio = BorrowedThread::CURRENT_THREAD
        .get_thread_priority()
        .expect("Failed to get thread priority");

    // camera worker will continuously pull frames from the camera
    let view_size = ViewSize::TopLCD;
    let (width, height) = view_size.into();
    let (_camera_worker, buffer_output) =
        camera::CameraWorker::new(view_size, main_thread_prio - 3)
            .expect("Failed to initialize camera worker");

    // qr worker will continuously try to decode QR codes from the frames
    let (_qr_worker, qr_handle) =
        qr::QrWorker::new(view_size, buffer_output.clone(), main_thread_prio + 3)
            .expect("Failed to initialize QR worker");

    // y2r context will help us convert YUV frames to RGB
    // this will happen synchronously!
    let mut y2r = camera::Y2rContext::new(
        buffer_output,
        view_size.into(),
        view_size.into(),
        y2r::StandardCoefficient::ItuRBt709,
    )
    .expect("Failed to initialize y2r context");

    bottom_screen.select();

    let (ui_sender, ui_receiver) = async_channel::bounded(8);

    let ui_handle = UiHandle::new(ui_sender);

    // flow thread will run the async logic of the application
    // it will send messages to the UI (main) thread and run HTTP requests
    let _flow_thread = std::thread::Builder::new()
        .stack_size(32 * 1024)
        .spawn({
            let ui_handle = ui_handle.clone();
            move || {
                BorrowedThread::CURRENT_THREAD
                    .set_thread_priority(main_thread_prio - 1)
                    .unwrap();
                flow::exec_flow(ui_handle, qr_handle)
            }
        })
        .unwrap();

    // we use STDOUT as our faux UI
    // Ideally we would design some nice-looking assets and use them instead of just boring text
    println!("Press Start to exit");

    let mut pending_prompt: Option<oneshot::Sender<()>> = None;
    let mut top_mode = TopMode::Viewfinder;

    while apt.main_loop() {
        hid.scan_input();
        let keys = hid.keys_down();
        if keys.contains(KeyPad::START) {
            break;
        }
        if keys.contains(KeyPad::A) | keys.contains(KeyPad::B) {
            if let Some(prompt) = pending_prompt.take() {
                prompt.send(()).unwrap();
            }
        }

        // handle UI commands
        // currently all we do is just pring stuff to the bottom screen
        // if we go into the GUI era, those would update widget state
        // not sure how I am going to render it yet
        // maybe it'll be citro3d + citro2d, or imgui
        while let Some(command) = match ui_receiver.try_recv() {
            Ok(c) => Some(c),
            Err(async_channel::TryRecvError::Empty) => None,
            Err(async_channel::TryRecvError::Closed) => panic!("UI channel closed"),
        } {
            match command {
                UiCommand::NotifyError(e) => {
                    println!("Error: {}", e);
                }
                UiCommand::PromptRestart(e, prompt) => {
                    println!("Error: {}\nPress A or B to retry", e);
                    pending_prompt = Some(prompt);
                }
                UiCommand::AskToScanLogin => {
                    top_mode = TopMode::Viewfinder;
                    println!(
                        "Please scan the login QR\n  at https://{}/ConnectMobile",
                        api::BAAM_HOST
                    );
                }
                UiCommand::AskToScanChallenge => {
                    top_mode = TopMode::Viewfinder;
                    println!("Please scan the attendance QR");
                }
                UiCommand::PromptSuccess(
                    session_name,
                    attendance_snippet,
                    your_username,
                    motd,
                    prompt,
                ) => {
                    top_mode = TopMode::Checkmark;
                    println!("Successfully marked in\n   {}\n", session_name);
                    for attendance in attendance_snippet {
                        println!(
                            " {} {}",
                            if attendance == your_username {
                                "*"
                            } else {
                                " "
                            },
                            attendance
                        )
                    }

                    if let Some(motd) = motd {
                        println!("\nMessage of the day:\n{}", motd);
                    }

                    pending_prompt = Some(prompt);
                }
                UiCommand::SetUsername(username) => {
                    println!(
                        "Now logged in\n  as {}",
                        username
                            .as_ref()
                            .map(|v| v.as_str())
                            .unwrap_or("<anonymous>")
                    )
                }
                UiCommand::SetNetState(_) => {}
                UiCommand::FinishedProcessing(_) => {}
            }
        }

        // update the viewfinder
        let rgb = y2r.get_rgb_buffer().expect("Failed to get RGB buffer");
        // alternatively, we could point to the GPU to the RGB buffer and use it as a texture

        let frame_buffer = top_screen.raw_framebuffer();

        match top_mode {
            TopMode::Viewfinder => {
                rotate_image_to_screen(
                    rgb,
                    top_screen.raw_framebuffer().ptr,
                    width as usize,
                    height as usize,
                );
            }
            TopMode::Checkmark => unsafe {
                frame_buffer
                    .ptr
                    .copy_from(TOP_CHECK.as_ptr(), TOP_CHECK.len());
            },
        }

        top_screen.flush_buffers();
        top_screen.swap_buffers();

        gfx.wait_for_vblank();
    }
}

// The 3DS' screens are 2 vertical LCD panels rotated by 90 degrees.
// As such, we'll need to write a "vertical" image to the framebuffer to have it displayed properly.
// This functions rotates an horizontal image by 90 degrees to the right.
// This function is only supposed to be used in this example. In a real world application, the program should use the GPU to draw to the screen.
fn rotate_image_to_screen(src: &[u8], framebuf: *mut u8, width: usize, height: usize) {
    for j in 0..height {
        for i in 0..width {
            // Y-coordinate of where to draw in the frame buffer
            // Height must be esclusive of the upper end (otherwise, we'd be writing to the pixel one column to the right when having j=0)
            let draw_y = (height - 1) - j;
            // X-coordinate of where to draw in the frame buffer
            let draw_x = i;

            // Index of the pixel to draw within the image buffer
            let read_index = (j * width + i) * 2;

            // Initial index of where to draw in the frame buffer based on y and x coordinates
            let draw_index = (draw_x * height + draw_y) * 2; // This 2 stands for the number of bytes per pixel (16 bits)

            unsafe {
                // We'll work with pointers since the framebuffer is a raw pointer regardless.
                // The offsets are completely safe as long as the width and height are correct.
                let pixel_pointer = framebuf.add(draw_index);
                pixel_pointer.copy_from(src.as_ptr().add(read_index), 2);
            }
        }
    }
}
