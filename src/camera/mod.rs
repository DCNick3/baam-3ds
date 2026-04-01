mod y2r_ctx;

use crate::spmc_buffer;
use anyhow::{Context, Result};
use ctru::error::ResultCode;
use ctru::linear::LinearAllocator;
use ctru::select;
use ctru::services::cam;
use ctru::services::cam::{Camera, FrameRate, Trimming, ViewSize, WhiteBalance};
use ctru::services::svc::handle::{
    BorrowedEvent, BorrowedThread, EventResetType, OwnedEvent, OwnedInterruptEvent,
};
use std::mem::ManuallyDrop;
use tracing::{error, info, warn};
pub use y2r_ctx::Y2rContext;

// TODO: ideally, upstream ctru-rs should have those
// it'll have to be polished to be up to the upstream's standard though
trait CameraExt: Camera {
    fn set_transfer_bytes_to_max(&mut self) -> Result<u32, ctru::Error> {
        let final_view = self.final_view_size();
        let transfer_unit = unsafe {
            let mut transfer_unit = 0;

            ResultCode(ctru_sys::CAMU_GetMaxBytes(
                &mut transfer_unit,
                final_view.0,
                final_view.1,
            ))?;

            transfer_unit
        };

        unsafe {
            ResultCode(ctru_sys::CAMU_SetTransferBytes(
                self.port_as_raw(),
                transfer_unit,
                final_view.0,
                final_view.1,
            ))?;
        };

        Ok(transfer_unit)
    }

    fn activate(&mut self) -> Result<(), ctru::Error> {
        unsafe { ResultCode(ctru_sys::CAMU_Activate(self.camera_as_raw()))? }

        Ok(())
    }

    // TODO: this should probably not be a method on the camera
    fn deactivate(&mut self) -> Result<(), ctru::Error> {
        unsafe { ResultCode(ctru_sys::CAMU_Activate(ctru_sys::SELECT_NONE.into()))? };

        Ok(())
    }

    fn clear_buffer(&mut self) -> Result<(), ctru::Error> {
        unsafe { ResultCode(ctru_sys::CAMU_ClearBuffer(self.port_as_raw()))? };

        Ok(())
    }

    fn start_capture(&mut self) -> Result<(), ctru::Error> {
        unsafe { ResultCode(ctru_sys::CAMU_StartCapture(self.port_as_raw()))? };

        Ok(())
    }

    fn stop_capture(&mut self) -> Result<(), ctru::Error> {
        unsafe { ResultCode(ctru_sys::CAMU_StopCapture(self.port_as_raw()))? };

        Ok(())
    }

    fn get_buffer_error_event(&mut self) -> Result<OwnedInterruptEvent, ctru::Error> {
        let mut handle = 0;

        unsafe {
            ResultCode(ctru_sys::CAMU_GetBufferErrorInterruptEvent(
                &mut handle as *mut _,
                self.port_as_raw(),
            ))?;
        }

        Ok(unsafe { OwnedInterruptEvent::owned_from_raw(handle) })
    }
    fn get_vsync_event(&mut self) -> Result<OwnedInterruptEvent, ctru::Error> {
        let mut handle = 0;

        unsafe {
            ResultCode(ctru_sys::CAMU_GetVsyncInterruptEvent(
                &mut handle as *mut _,
                self.port_as_raw(),
            ))?;
        }

        Ok(unsafe { OwnedInterruptEvent::owned_from_raw(handle) })
    }
    unsafe fn set_receiving(
        &mut self,
        data: &mut [u8],
        transfer_size: i16,
    ) -> Result<OwnedInterruptEvent, ctru::Error> {
        let mut handle = 0;

        unsafe {
            ResultCode(ctru_sys::CAMU_SetReceiving(
                &mut handle,
                data.as_mut_ptr() as *mut std::ffi::c_void,
                self.port_as_raw(),
                data.len() as u32,
                transfer_size,
            ))?;
        }

        Ok(unsafe { OwnedInterruptEvent::owned_from_raw(handle) })
    }
}

impl<T: Camera> CameraExt for T {}

pub type YuvBuffer = Box<[u8], LinearAllocator>;

const CONSUMER_COUNT: usize = 6;

pub fn make_yuv_buffers(
    buffer_size: usize,
) -> (
    spmc_buffer::Input<YuvBuffer>,
    spmc_buffer::Output<YuvBuffer>,
) {
    spmc_buffer::spmc_buffer(CONSUMER_COUNT, || unsafe {
        std::boxed::Box::<[u8], _>::new_zeroed_slice_in(buffer_size, LinearAllocator).assume_init()
    })
}

fn get_cam(service: &mut cam::Cam) -> &mut impl Camera {
    &mut service.outer_right_cam
}

struct CameraContext {
    buffer_input: spmc_buffer::Input<YuvBuffer>,
    service: cam::Cam,
    transfer_unit: u32,
    buffer_error_event: OwnedInterruptEvent,
    vsync_event: OwnedInterruptEvent,
    receive_event: Option<OwnedInterruptEvent>,
}

impl CameraContext {
    pub fn new(view_size: ViewSize, buffer_input: spmc_buffer::Input<YuvBuffer>) -> Result<Self> {
        let mut service = cam::Cam::new().context("Initializing the cam service")?;

        let camera = get_cam(&mut service);

        camera
            .set_view_size(view_size)
            .context("Setting view size")?;
        camera
            .set_trimming(Trimming::Off)
            .context("Setting trimming")?;

        camera
            .set_output_format(cam::OutputFormat::Yuv422)
            .context("Setting output format")?;

        let transfer_unit = camera
            .set_transfer_bytes_to_max()
            .context("Setting transfer bytes to max")?;

        camera
            .set_noise_filter(true)
            .context("Setting noise filter")?;
        camera
            .set_auto_exposure(true)
            .context("Setting auto exposure")?;
        camera
            .set_white_balance(WhiteBalance::Auto)
            .context("Setting white balance")?;

        camera
            .set_frame_rate(FrameRate::Fps30)
            .context("Setting frame rate")?;

        camera.activate().context("Activating camera")?;

        let buffer_error_event = camera
            .get_buffer_error_event()
            .context("Getting buffer error event")?;
        let vsync_event = camera.get_vsync_event().context("Getting vsync event")?;

        Ok(Self {
            buffer_input,
            service,
            transfer_unit,
            buffer_error_event,
            vsync_event,
            receive_event: None,
        })
    }

    unsafe fn start_capture(&mut self) -> Result<()> {
        get_cam(&mut self.service)
            .clear_buffer()
            .context("Clearing buffer")?;

        unsafe {
            self.setup_receiving_buffer()
                .context("Starting frame capture")?;
        }

        get_cam(&mut self.service)
            .start_capture()
            .context("Starting capture")?;

        Ok(())
    }

    fn stop_capture(&mut self) -> Result<()> {
        let camera = get_cam(&mut self.service);

        camera.stop_capture().context("Stopping capture")?;

        const TIMEOUT: u32 = 200;
        let mut attempt = 0;
        while camera.is_busy().context("Checking if camera is busy")? {
            std::thread::sleep(std::time::Duration::from_millis(1));
            attempt += 1;
            if attempt > TIMEOUT {
                error!("Timeout waiting for camera to stop capture");
                break;
            }
        }

        camera.clear_buffer().context("Clearing buffer")?;

        Ok(())
    }

    unsafe fn setup_receiving_buffer(&mut self) -> Result<()> {
        let camera = get_cam(&mut self.service);
        let input_buffer = self.buffer_input.input_buffer_mut();

        self.receive_event = None;

        // SAFETY: we won't touch input_buffer again until the interrupt is fired or the transfer is cancelled
        let receive_event =
            unsafe { camera.set_receiving(input_buffer, self.transfer_unit.try_into().unwrap()) }
                .context("Starting camera frame capture")?;

        self.receive_event = Some(receive_event);

        Ok(())
    }

    unsafe fn publish_receiving_buffer(&mut self) {
        self.buffer_input.publish();
    }
}

impl Drop for CameraContext {
    fn drop(&mut self) {
        let camera = &mut self.service.outer_right_cam;
        camera.stop_capture().expect("Failed to stop capture");
        camera.deactivate().expect("Failed to deactivate camera");
    }
}

fn camera_worker_fn(
    view_size: ViewSize,
    buffer_input: spmc_buffer::Input<YuvBuffer>,
    shutdown_event: OwnedEvent,
) {
    let mut context =
        CameraContext::new(view_size, buffer_input).expect("Failed to create camera context");

    unsafe { context.start_capture() }.expect("Failed to start capture");

    loop {
        select! {
            context.receive_event.as_ref().unwrap() => {
                unsafe {
                    context.publish_receiving_buffer();

                    context.setup_receiving_buffer().expect("Failed to setup receiving buffer");
                }
            },
            context.buffer_error_event => {
                warn!("Got a buffer error signal! Restarting capture");
                unsafe { context.start_capture() }.expect("Failed to start capture");
            },
            shutdown_event => {
                break;
            },
        }
    }

    context.stop_capture().expect("Failed to stop capture");
    let camera = get_cam(&mut context.service);
    camera.deactivate().expect("Failed to deactivate camera");
}

pub struct CameraWorker {
    shutdown_event: BorrowedEvent<'static>,
    thread: ManuallyDrop<std::thread::JoinHandle<()>>,
}

impl CameraWorker {
    pub fn new(
        view_size: ViewSize,
        priority: i32,
    ) -> Result<(Self, spmc_buffer::Output<YuvBuffer>)> {
        let event =
            OwnedEvent::new_event(EventResetType::Sticky).context("Failed to create event")?;
        let borrowed_event = unsafe { event.borrowed_static() };

        let view_size_int: (i16, i16) = view_size.into();
        let buffer_size = view_size_int.0 as usize * view_size_int.1 as usize * 2; // both camera formats (YUV422 and RGB16 are 16bpp)

        let (buffer_input, buffer_output) = make_yuv_buffers(buffer_size);

        let thread = std::thread::Builder::new()
            // .priority(prio - 3)
            .stack_size(32 * 1024)
            .name("CameraWorker".to_string())
            .spawn(move || {
                BorrowedThread::CURRENT_THREAD
                    .set_thread_priority(priority)
                    .unwrap();

                camera_worker_fn(view_size, buffer_input, event)
            })
            .context("Failed to spawn camera thread")?;

        Ok((
            Self {
                shutdown_event: borrowed_event,
                thread: ManuallyDrop::new(thread),
            },
            buffer_output,
        ))
    }
}

impl Drop for CameraWorker {
    fn drop(&mut self) {
        info!("Dropping CameraWorker...");

        self.shutdown_event.signal();
        info!("Joining CameraWorker...");
        unsafe {
            ManuallyDrop::take(&mut self.thread)
                .join()
                .expect("Failed to join camera thread")
        }
    }
}
