use crate::camera::YuvBuffer;
use crate::spmc_buffer;
use anyhow::{Context, Result, bail};
use ctru::linear::LinearAllocator;
use ctru::services::svc::handle::OwnedInterruptEvent;
use ctru::services::y2r;
use ctru::services::y2r::{ConversionParams, Y2r};

pub struct Y2rContext {
    yuv_receiver: spmc_buffer::Output<YuvBuffer>,
    camera_view_size: (i16, i16),
    texture_size: (i16, i16),
    service: Y2r,
    transfer_end_event: OwnedInterruptEvent,
    rgb_buffer: Box<[u16], LinearAllocator>,
}

impl Y2rContext {
    pub fn new(
        yuv_receiver: spmc_buffer::Output<YuvBuffer>,
        camera_view_size: (i16, i16),
        texture_size: (i16, i16),
        standard_coefficient: y2r::StandardCoefficient,
    ) -> Result<Self> {
        let mut service = Y2r::new().context("Initializing the y2r service")?;

        service.stop_conversion().context("Stopping conversion")?;
        while service.is_busy().context("Checking y2r busy status")? {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        let output_format = y2r::OutputFormat::Rgb16_565;

        service
            .set_conversion_params(ConversionParams {
                input_format: y2r::InputFormat::Yuv422Batch,
                output_format,
                rotation: y2r::Rotation::None,
                // block_alignment: y2r::BlockAlignment::Block8by8,
                block_alignment: y2r::BlockAlignment::Line,
                input_line_width: camera_view_size.0,
                input_lines: camera_view_size.1,
                standard_coefficient,
                alpha: 0,
            })
            .context("Setting conversion params")?;

        let transfer_end_event = service
            .enable_transfer_end_interrupt()
            .context("Enabling transfer end interrupt")?;

        // let rgb_bpp = match output_format {
        //     y2r::OutputFormat::Rgb32 => 4,
        //     y2r::OutputFormat::Rgb24 => 3,
        //     y2r::OutputFormat::Rgb16_555 => 2,
        //     y2r::OutputFormat::Rgb16_565 => 2,
        // };

        let rgb_buffer_size = texture_size.0 as usize * texture_size.1 as usize;
        let rgb_buffer = unsafe {
            Box::<[u16], _>::new_zeroed_slice_in(rgb_buffer_size, LinearAllocator).assume_init()
        };

        Ok(Self {
            yuv_receiver,
            camera_view_size,
            texture_size,
            service,
            transfer_end_event,
            rgb_buffer,
        })
    }

    // I think it would be nicer to do this asyncly, otherwise we just block the render thread
    pub fn get_rgb_buffer(&mut self) -> Result<&[u16]> {
        if !self.yuv_receiver.update() {
            return Ok(&self.rgb_buffer);
        }
        let buffer = self.yuv_receiver.output_buffer_mut();

        let rgb_line_size = self.texture_size.0 * 2;
        let rgb_line_skip = (self.texture_size.0 - self.camera_view_size.0) * 2;

        let mut attempt = 0;
        loop {
            unsafe {
                self.service
                    .set_sending_yuyv(&buffer, self.camera_view_size.0 * 2, 0)
                    .context("Setting sending yuyv")?;
                self.service
                    .set_receiving(
                        bytemuck::cast_slice_mut(&mut self.rgb_buffer),
                        rgb_line_size * 8,
                        rgb_line_skip * 8,
                    )
                    .context("Setting receiving")?;
                self.service
                    .start_conversion()
                    .context("Starting conversion")?;
            }

            if self
                .transfer_end_event
                .wait_for(std::time::Duration::from_millis(10))
                .is_ok()
            {
                return Ok(&self.rgb_buffer);
            } else {
                attempt += 1;
                if attempt >= 2 {
                    bail!("Failed to convert YUV to RGB");
                }
            }
        }
    }
}
