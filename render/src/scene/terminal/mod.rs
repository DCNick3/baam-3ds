mod buffer;

use crate::texture::Tex2Data;
use crate::timer::{BackoffTimer, PeriodicSkippingTimer};
use crate::{SceneRequest, blend, effects, texture_data};
pub use buffer::ScreenBuffer;
use imgref::{ImgExtMut, ImgRefMut};
use std::collections::VecDeque;

pub enum TerminalCommand {
    WriteChar(char),
    Newline,
}

pub struct TerminalState {
    buffer: ScreenBuffer,
    command_queue: VecDeque<TerminalCommand>,
    command_timer: BackoffTimer,
    cursor_timer: PeriodicSkippingTimer,
    cursor_position: usize,
}

impl TerminalState {
    pub fn new() -> Self {
        Self {
            buffer: ScreenBuffer::new(texture_data::TEX_SCREEN_LOGO),
            command_queue: VecDeque::new(),
            cursor_timer: PeriodicSkippingTimer::new(500),
            command_timer: BackoffTimer::new(),
            cursor_position: 0,
        }
    }

    pub fn push_string(&mut self, s: &str) {
        for c in s.chars() {
            if c == '\n' {
                self.push_command(TerminalCommand::Newline)
            } else {
                self.push_command(TerminalCommand::WriteChar(c))
            }
        }
    }

    pub fn push_command(&mut self, command: TerminalCommand) {
        self.command_queue.push_back(command);
    }

    fn update(&mut self, tick: u32) {
        while self.command_timer.ready(tick) {
            if let Some(command) = self.command_queue.pop_front() {
                self.cursor_timer.reset(tick);
                let delay = match &command {
                    TerminalCommand::WriteChar(_) => 15,
                    TerminalCommand::Newline => 150,
                };
                self.command_timer.delay(delay);

                match command {
                    TerminalCommand::WriteChar(c) => {
                        if self.cursor_position < buffer::WIDTH_CELLS {
                            self.buffer.blit_cell(
                                self.cursor_position,
                                buffer::HEIGHT_CELLS - 1,
                                c,
                            );
                        }
                        self.cursor_position += 1;
                    }
                    TerminalCommand::Newline => {
                        if self.cursor_position < buffer::WIDTH_CELLS {
                            self.buffer.blit_cell(
                                self.cursor_position,
                                buffer::HEIGHT_CELLS - 1,
                                ' ',
                            );
                        }
                        self.buffer.scroll();
                        self.cursor_position = 0;
                    }
                }
            } else {
                self.command_timer.reset(tick);
                break;
            }
        }

        if self.cursor_timer.update(tick) {
            if self.cursor_position < buffer::WIDTH_CELLS {
                self.buffer
                    .invert_cell(self.cursor_position, buffer::HEIGHT_CELLS - 1);
            }
        }
    }

    pub fn draw(&mut self, mut dest: ImgRefMut<u16>, request: &SceneRequest) {
        self.update(request.tick);

        // display contents
        // let DISPLAY_FG = 0x0715;
        const DISPLAY_FG: u16 = 0x17e9;
        const DISPLAY_GLOW: u16 = blend::blend_rgb565(0x0000, DISPLAY_FG, 0x50);
        const DISPLAY_ACTIVE_BG: u16 = blend::blend_rgb565(0x0000, DISPLAY_FG, 0x20);
        const DISPLAY_PASSIVE_BG: u16 = blend::blend_rgb565(0x0000, DISPLAY_FG, 0x18);

        static PALETTE_REGULAR: [u16; 4] = [
            DISPLAY_PASSIVE_BG,
            DISPLAY_ACTIVE_BG,
            DISPLAY_GLOW,
            DISPLAY_FG,
        ];

        // used to implement the "scanlines" effect
        const DIM_ALPHA: u8 = 0xa0;
        static PALETTE_DIM: [u16; 4] = [
            blend::blend_rgb565(0x0000, PALETTE_REGULAR[0], DIM_ALPHA),
            blend::blend_rgb565(0x0000, PALETTE_REGULAR[1], DIM_ALPHA),
            blend::blend_rgb565(0x0000, PALETTE_REGULAR[2], DIM_ALPHA),
            blend::blend_rgb565(0x0000, PALETTE_REGULAR[3], DIM_ALPHA),
        ];

        let mut display_contents = dest.sub_image_mut(13, 12, 196, 224);

        let buffer = self.buffer.as_tex2();

        #[inline]
        fn sample_buffer(buffer: &Tex2Data, x: usize, y: usize) -> u8 {
            if x >= 4 && x < 4 + 216 && y >= 7 && y < 7 + 182 {
                unsafe { buffer.get_value_at_unchecked(x - 4, y - 7) }
            } else {
                0x0
            }
        }

        let dst_w = display_contents.width();
        let dst_stride = display_contents.stride();
        let dst_h = display_contents.height();

        for dst_x in 0..dst_w {
            let palette = if dst_x % 3 < 2 {
                &PALETTE_REGULAR
            } else {
                &PALETTE_DIM
            };

            for dst_y in 0..dst_h {
                let src_x = dst_y;
                let src_y = dst_w - dst_x - 1;

                let index = if src_x >= 3 && src_x < 4 + 217 && src_y >= 7 && src_y < 7 + 182 {
                    let index_left = sample_buffer(&buffer, src_x - 1, src_y);
                    let index = sample_buffer(&buffer, src_x, src_y);
                    let index_right = sample_buffer(&buffer, src_x + 1, src_y);

                    if index == 3 {
                        3
                    } else if index_left == 3 || index_right == 3 {
                        2
                    } else {
                        1
                    }
                } else {
                    0
                };

                *unsafe {
                    display_contents
                        .buf_mut()
                        .get_unchecked_mut(dst_y * dst_stride + dst_x)
                } = palette[index];
            }
        }

        // effects::lineblur(display_contents.as_mut());
        // effects::scanlines(display_contents.as_mut(), 0xa0);
    }
}
