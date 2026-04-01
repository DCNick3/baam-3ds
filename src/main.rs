#![feature(allocator_api)]
// #![feature(unsafe_cell_access)]

mod api;
mod camera;
mod flow;
mod qr;
mod settings_storage;
mod spmc_buffer;
mod ui;

use crate::ui::{SystemState, UiCommand, UiHandle};
use ctru::prelude::*;
use ctru::services::cam::ViewSize;
use ctru::services::gfx::{Flush, RawFrameBuffer, Screen, Swap};
use ctru::services::gspgpu::FramebufferFormat;
use ctru::services::svc::handle::BorrowedThread;
use ctru::services::y2r;
use imgref::{ImgExtMut, ImgRefMut};
use render::{
    BreadcrumbRequest, CameraQrTestRequest, CameraRequest, NetworkRequest, SceneRequest, SceneState,
};
use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tracing::{debug, info};

fn init_tracing(soc: &mut Soc) {
    if soc.redirect_to_3dslink(false, true).is_ok() {
        tracing_subscriber::fmt::Subscriber::builder()
            .with_writer(std::io::stderr)
            // .with_max_level(tracing::Level::DEBUG)
            .init();
    }
}

// 320x240px RGB565
// ffmpeg -vcodec png -i top_check.png -vf "transpose=1" -vcodec rawvideo -f rawvideo -pix_fmt rgb565 -y top_check.rgb
static TOP_CHECK: &[u8] = include_bytes!("../res/top_check.rgb");

/// Convert `ctru`'s [`RawFrameBuffer`] to a safe [`ImgRefMut`] object.
///
/// # Safety
///
/// - The provided framebuffer structure is valid
/// - The framebuffer is using a 2 bpp format (RGB565, presumably)
pub unsafe fn framebuffer_to_imgref(fb: RawFrameBuffer) -> ImgRefMut<u16> {
    let ptr = fb.ptr as *mut u16;
    let slice = unsafe { std::slice::from_raw_parts_mut(ptr, fb.width * fb.height) };

    ImgRefMut::new(slice, fb.width, fb.height)
}

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
    // let bottom_screen = Console::new(gfx.bottom_screen.borrow_mut());

    let mut bottom_screen = gfx.bottom_screen.borrow_mut();

    bottom_screen.set_double_buffering(true);
    bottom_screen.set_framebuffer_format(FramebufferFormat::Rgb565);

    let main_thread_prio = BorrowedThread::CURRENT_THREAD
        .get_thread_priority()
        .expect("Failed to get thread priority");

    let system_state = Arc::new(SystemState::new());

    // camera worker will continuously pull frames from the camera
    let view_size = ViewSize::TopLCD;
    let (width, height) = view_size.into();
    let (_camera_worker, buffer_output) =
        camera::CameraWorker::new(view_size, main_thread_prio - 3)
            .expect("Failed to initialize camera worker");

    // qr worker will continuously try to decode QR codes from the frames
    let (_qr_worker, qr_handle) = qr::QrWorker::new(
        system_state.clone(),
        view_size,
        buffer_output.clone(),
        main_thread_prio + 3,
    )
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

    let start_time = Instant::now();

    let mut scene_state = SceneState::new();
    scene_state.terminal.push_string("Initializing...\n");

    let mut breadcrumb_state = BreadcrumbRequest::Login(true);

    let (ui_sender, ui_receiver) = async_channel::bounded(8);

    let ui_handle = UiHandle::new(ui_sender);

    // flow thread will run the async logic of the application
    // it will send messages to the UI (main) thread and run HTTP requests
    let _flow_thread = std::thread::Builder::new()
        .stack_size(32 * 1024)
        .spawn({
            let ui_handle = ui_handle.clone();
            let system_state = system_state.clone();
            move || {
                BorrowedThread::CURRENT_THREAD
                    .set_thread_priority(main_thread_prio - 1)
                    .unwrap();
                flow::exec_flow(ui_handle, system_state, qr_handle)
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
                scene_state.terminal.push_string("\n");
            }
        }
        // if keys.contains(KeyPad::SELECT) {
        //     scene_state = SceneState::new();
        //     scene_state
        //         .terminal
        //         .push_string("Initializing...\nLogin: Nikita Strygin\nДаттебайо! 日本語\n")
        // }

        let mut error_reported = false;

        while let Some(command) = match ui_receiver.try_recv() {
            Ok(c) => Some(c),
            Err(async_channel::TryRecvError::Empty) => None,
            Err(async_channel::TryRecvError::Closed) => panic!("UI channel closed"),
        } {
            match command {
                UiCommand::NotifyError(e) => {
                    error_reported = true;
                    scene_state.terminal.push_string(&format!("Error: {}\n", e));
                }
                UiCommand::PromptRestart(e, prompt) => {
                    error_reported = true;
                    // println!("Error: {}\nPress A or B to retry", e);
                    scene_state
                        .terminal
                        .push_string(&format!("Error: {}\nRetry? [A/B]", e));
                    pending_prompt = Some(prompt);
                }
                UiCommand::AskToScanLogin => {
                    top_mode = TopMode::Viewfinder;
                    scene_state.terminal.push_string(&format!(
                        "Scan login QR\n {}/ConnectMobile\n",
                        api::BAAM_HOST
                    ));
                    // println!(
                    //     "Please scan the login QR\n  at https://{}/ConnectMobile",
                    //     api::BAAM_HOST
                    // );
                }
                UiCommand::AskToScanChallenge => {
                    top_mode = TopMode::Viewfinder;
                    scene_state.terminal.push_string("Scan attendance QR\n");
                    // println!("Please scan the attendance QR");
                }
                UiCommand::PromptSuccess(
                    session_name,
                    attendance_snippet,
                    your_username,
                    motd,
                    prompt,
                ) => {
                    top_mode = TopMode::Checkmark;
                    let mut message = String::new();

                    writeln!(message, "Marked in\n {}", session_name).unwrap();
                    for attendance in attendance_snippet {
                        let attendance_display =
                            if let Some((name, _domain)) = attendance.split_once("@") {
                                name
                            } else {
                                &attendance
                            };

                        writeln!(
                            message,
                            " {} {}",
                            if attendance == your_username {
                                "*"
                            } else {
                                " "
                            },
                            attendance_display
                        )
                        .unwrap()
                    }

                    if let Some(motd) = motd {
                        writeln!(message, "\nMessage of the day:\n{}", motd).unwrap();
                    }

                    write!(message, "Again? [A/B]").unwrap();

                    scene_state.terminal.push_string(&message);

                    pending_prompt = Some(prompt);
                }
                UiCommand::SetUsername(username) => {
                    scene_state.terminal.push_string(&format!(
                        "Logged in as\n {}\n",
                        username.as_ref().map(|v| v.as_str()).unwrap_or("<unknown>")
                    ));

                    // println!(
                    //     "Now logged in\n  as {}",
                    //     username
                    //         .as_ref()
                    //         .map(|v| v.as_str())
                    //         .unwrap_or("<anonymous>")
                    // )
                }
                UiCommand::SetBreadcrumbState(state) => {
                    breadcrumb_state = state;
                }
            }
        }

        // TOP SCREEN
        // update the viewfinder
        let rgb = y2r.get_rgb_buffer().expect("Failed to get RGB buffer");

        let top_frame_buffer = top_screen.raw_framebuffer();

        match top_mode {
            TopMode::Viewfinder => {
                rotate_image_to_screen(rgb, top_frame_buffer.ptr, width as usize, height as usize);
            }
            TopMode::Checkmark => unsafe {
                top_frame_buffer
                    .ptr
                    .copy_from(TOP_CHECK.as_ptr(), TOP_CHECK.len());
            },
        }

        // BOTTOM SCREEN
        let request = SceneRequest {
            tick: start_time.elapsed().as_millis() as u32,
            // breadcrumb_request: BreadcrumbRequest::Login(true),
            breadcrumb_request: breadcrumb_state,
            // breadcrumb_request: BreadcrumbRequest::Mark(false),
            // breadcrumb_request: BreadcrumbRequest::Mark(true),
            // breadcrumb_request: BreadcrumbRequest::Success,
            network_request: NetworkRequest {
                active: system_state.net_state.load(Ordering::Relaxed),
                error: error_reported,
            },
            camera_request: CameraRequest {
                active: system_state
                    .qr_processing_pulse
                    .swap(false, Ordering::Relaxed),
                scan: match system_state.qr_test_pulse.swap(0, Ordering::Relaxed) {
                    2 => CameraQrTestRequest::Rejected,
                    1 => CameraQrTestRequest::Accepted,
                    0 | _ => CameraQrTestRequest::None,
                },
            },
        };

        // let start = Instant::now();
        // render to the "regular" framebuffer
        let mut bottom_framebuffer =
            unsafe { framebuffer_to_imgref(bottom_screen.raw_framebuffer()) };
        scene_state.draw(bottom_framebuffer.as_mut(), &request);
        // let elapsed = start.elapsed();
        // debug!("Bottom render: {}us", elapsed.as_micros(),);

        // let mut fb = unsafe { framebuffer_to_imgref(top_frame_buffer) };
        //
        // let net_state = system_state.net_state.load(Ordering::Relaxed);
        // let qr_processing_state = system_state
        //     .qr_processing_pulse
        //     .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |_| Some(false))
        //     .unwrap();

        // flop top & bottom screen at roughly the same time
        top_screen.flush_buffers();
        top_screen.swap_buffers();
        bottom_screen.flush_buffers();
        bottom_screen.swap_buffers();

        // framerate_counter += 1;
        // let frame_elapsed = framerate_timer.elapsed();
        // if frame_elapsed > Duration::from_secs(10) {
        //     let fps = framerate_counter as f32 / frame_elapsed.as_secs_f32();
        //     debug!("FPS: {:.02}", fps);
        //
        //     framerate_timer = Instant::now();
        //     framerate_counter = 0
        // }

        gfx.wait_for_vblank();
    }
}

// The 3DS' screens are 2 vertical LCD panels rotated by 90 degrees.
// As such, we'll need to write a "vertical" image to the framebuffer to have it displayed properly.
// This functions rotates an horizontal image by 90 degrees to the right.
// This function is only supposed to be used in this example. In a real world application, the program should use the GPU to draw to the screen.
fn rotate_image_to_screen(src: &[u16], framebuf: *mut u8, width: usize, height: usize) {
    // FIXME: idk, we can probably use `imgref` here to do the blit a bit more safely
    for j in 0..height {
        for i in 0..width {
            // Y-coordinate of where to draw in the frame buffer
            // Height must be esclusive of the upper end (otherwise, we'd be writing to the pixel one column to the right when having j=0)
            let draw_y = (height - 1) - j;
            // X-coordinate of where to draw in the frame buffer
            let draw_x = i;

            // Index of the pixel to draw within the image buffer
            let read_index = j * width + i;

            // Initial index of where to draw in the frame buffer based on y and x coordinates
            let draw_index = (draw_x * height + draw_y) * 2; // This 2 stands for the number of bytes per pixel (16 bits)

            unsafe {
                // We'll work with pointers since the framebuffer is a raw pointer regardless.
                // The offsets are completely safe as long as the width and height are correct.
                let pixel_pointer = framebuf.add(draw_index);
                pixel_pointer.copy_from(src.as_ptr().add(read_index) as *const u8, 2);
            }
        }
    }
}
