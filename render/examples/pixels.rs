use pixels::{Pixels, SurfaceTexture};
use render::{BreadcrumbRequest, CameraRequest, NetworkRequest, SceneRequest, SceneState};
use std::sync::Arc;
use std::time::Instant;
use tracing::error;
use winit::dpi::PhysicalSize;
use winit::{
    application::ApplicationHandler,
    error::EventLoopError,
    event::WindowEvent,
    event_loop::{ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowAttributes},
};

const WIDTH_PIXELS: usize = 320;
const HEIGHT_PIXELS: usize = 240;

// NB: to match the 3DS frame buffer, width and height are swapped
const WIDTH_3DS: usize = HEIGHT_PIXELS;
const HEIGHT_3DS: usize = WIDTH_PIXELS;

fn init_scene_state() -> SceneState {
    let mut scene_state = SceneState::new();

    scene_state
        .terminal
        .push_string("Login: Nikita Strygin\nДаттебайо! 日本語\n");

    scene_state
}

pub struct App {
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    frame_buffer_3ds: imgref::ImgVec<u16>,
    scene_state: SceneState,
    start_time: Instant,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = event_loop
            .create_window(
                WindowAttributes::default().with_inner_size(PhysicalSize::new(
                    WIDTH_PIXELS as u32 * 4,
                    HEIGHT_PIXELS as u32 * 4,
                )),
            )
            .unwrap();
        let window = Arc::new(window);
        self.window = Some(window.clone());

        self.pixels = {
            let (window_width, window_height) = window.inner_size().into();
            let surface_texture = SurfaceTexture::new(window_width, window_height, window.clone());
            match Pixels::new(WIDTH_PIXELS as u32, HEIGHT_PIXELS as u32, surface_texture) {
                Ok(pixels) => {
                    window.request_redraw();
                    Some(pixels)
                }
                Err(err) => {
                    error!("pixels::new: {:?}", err);
                    event_loop.exit();
                    None
                }
            }
        };
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                let uptime = self.start_time.elapsed();
                let ticks = uptime.as_millis() as u32;

                let request = SceneRequest {
                    tick: ticks,
                    // breadcrumb_request: BreadcrumbRequest::Login(true),
                    breadcrumb_request: BreadcrumbRequest::Login(false),
                    // breadcrumb_request: BreadcrumbRequest::Mark(false),
                    // breadcrumb_request: BreadcrumbRequest::Mark(true),
                    // breadcrumb_request: BreadcrumbRequest::Success,
                    network_request: NetworkRequest {
                        active: false,
                        error: false,
                    },
                    camera_request: CameraRequest {
                        active: false,
                        scan: false,
                    },
                };

                self.scene_state
                    .draw(self.frame_buffer_3ds.as_mut(), &request);

                // convert 3DS frame buffer to pixels frame buffer;
                // There are two transforms we want to apply:
                // 1. Convert color space from RGB565 to RGBA8888
                // 2. Rotate the frame buffer 90 degrees CCW
                let frame_pixels = self.pixels.as_mut().unwrap().frame_mut();
                let (frame_pixels, _) = frame_pixels.as_chunks_mut::<4>();

                let frame_3ds = self.frame_buffer_3ds.buf().as_slice();

                for (i_pixels, pixel) in frame_pixels.iter_mut().enumerate() {
                    let x_pixels = i_pixels % WIDTH_PIXELS;
                    let y_pixels = i_pixels / WIDTH_PIXELS;

                    let x_3ds = WIDTH_3DS - y_pixels - 1;
                    let y_3ds = x_pixels;
                    // let x_3ds = x_pixels;
                    // let y_3ds = y_pixels;

                    let i_3ds = y_3ds * WIDTH_3DS + x_3ds;

                    let rgb565 = frame_3ds[i_3ds];

                    let [r, g, b] = rgb565::Rgb565::from_rgb565(rgb565).to_rgb888_components();
                    let rgba = [r, g, b, 0xff];

                    *pixel = rgba;
                }

                if let Err(err) = self.pixels.as_ref().unwrap().render() {
                    error!("pixels.render: {:?}", err);
                    event_loop.exit();
                }
                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::Resized(size) => {
                if let Err(err) = self
                    .pixels
                    .as_mut()
                    .unwrap()
                    .resize_surface(size.width, size.height)
                {
                    error!("pixels.resize_surface: {:?}", err);
                    event_loop.exit()
                }
            }
            WindowEvent::KeyboardInput {
                device_id: _,
                event,
                is_synthetic: _,
            } => {
                if event.state.is_pressed()
                    && let PhysicalKey::Code(code) = event.physical_key
                {
                    match code {
                        KeyCode::Escape => {
                            event_loop.exit();
                        }
                        KeyCode::KeyR => {
                            self.scene_state = init_scene_state();
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

fn main() -> Result<(), EventLoopError> {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App {
        window: None,
        pixels: None,
        frame_buffer_3ds: imgref::ImgVec::new(
            vec![0u16; WIDTH_3DS * HEIGHT_3DS],
            WIDTH_3DS,
            HEIGHT_3DS,
        ),
        scene_state: init_scene_state(),
        start_time: Instant::now(),
    };
    event_loop.run_app(&mut app)
}
