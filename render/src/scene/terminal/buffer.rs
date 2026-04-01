use crate::texture::Tex2Data;
use crate::{encoding, texture_data};

// NOTE: unlike the final frame buffer, this is oriented "correctly" (X goes right, Y goes up)
const WIDTH: usize = 216;
const HEIGHT: usize = 182;
pub const WIDTH_CELLS: usize = 27;
pub const HEIGHT_CELLS: usize = 13;

const CELL_WIDTH: usize = texture_data::FONT_GLYPH_SIZE.0;
const CELL_HEIGHT: usize = texture_data::FONT_GLYPH_SIZE.1;
const BPP: usize = 2;

pub struct ScreenBuffer {
    // format is Tex2 (4 pixels per byte)
    data: [u8; WIDTH * HEIGHT / 4],
}

impl ScreenBuffer {
    pub fn new(initial: Tex2Data) -> Self {
        assert_eq!(initial.height, HEIGHT);
        assert_eq!(initial.width, WIDTH);

        Self {
            data: initial.data.try_into().unwrap(),
        }
    }

    pub fn scroll(&mut self) {
        for pixel_y in 0..HEIGHT - CELL_HEIGHT {
            let byte_dst_start = pixel_y * WIDTH * BPP / 8;
            let byte_src_start = byte_dst_start + CELL_HEIGHT * WIDTH * BPP / 8;
            self.data.copy_within(
                byte_src_start..byte_src_start + WIDTH * BPP / 8,
                byte_dst_start,
            );
        }

        for x in 0..WIDTH_CELLS {
            self.blit_cell(x, HEIGHT_CELLS - 1, ' ');
        }
    }

    pub fn invert_cell(&mut self, cell_x: usize, cell_y: usize) {
        let cell_start_y = cell_y * CELL_HEIGHT;

        for buffer_y in cell_start_y..cell_start_y + CELL_HEIGHT {
            let byte_start = (buffer_y * WIDTH + cell_x * CELL_WIDTH) * BPP / 8;

            self.data[byte_start] ^= 0xff;
            self.data[byte_start + 1] ^= 0xff;
        }
    }

    pub fn blit_cell(&mut self, cell_x: usize, cell_y: usize, char: char) {
        let cell_start_y = cell_y * CELL_HEIGHT;

        let cell_range_y = cell_start_y..cell_start_y + CELL_HEIGHT;

        let cp866_code = encoding::CP866_ENCODER.encode_char(char).unwrap_or(254);

        let atlas_pos_x = cp866_code as usize % texture_data::FONT_GLYPHS_PER_ROW;
        let atlas_pos_y = cp866_code as usize / texture_data::FONT_GLYPHS_PER_ROW;

        let atlas_x = atlas_pos_x * CELL_WIDTH;
        let atlas_range_y = atlas_pos_y * CELL_HEIGHT..(atlas_pos_y + 1) * CELL_HEIGHT;

        for (atlas_y, buffer_y) in atlas_range_y.zip(cell_range_y) {
            let tex_font = &texture_data::TEX_FONT;
            let row_value_tex1 = tex_font.data[(atlas_y * tex_font.width + atlas_x) / 8];

            const {
                assert!(CELL_WIDTH % BPP == 0);
                assert!(CELL_WIDTH == 8);
            }

            let row_value_tex2 = double_up_bits(row_value_tex1);

            let byte_start = (buffer_y * WIDTH + cell_x * CELL_WIDTH) * BPP / 8;

            [self.data[byte_start], self.data[byte_start + 1]] = row_value_tex2.to_le_bytes();
        }
    }

    pub fn as_tex2(&self) -> Tex2Data<'_> {
        Tex2Data {
            width: WIDTH,
            height: HEIGHT,
            data: &self.data,
        }
    }
}

/// Double up bits of a byte. Used to implement an efficient Tex1 -> Tex2 blit
fn double_up_bits(x: u8) -> u16 {
    let mut x = x as u16;

    // Step 1: Spread bits apart.
    // We want to move bit `i` to position `2*i`.
    // This is equivalent to interleaving the bits with 0s.

    // Separate bits into groups of 2, expanding distance.
    // Example: `1101` -> groups `11` and `01`
    // Move high nibble up by 4 slots.
    x = (x | (x << 4)) & 0x0F0F;

    // Separate bits into groups of 4.
    // Move high chunks up by 2 slots.
    x = (x | (x << 2)) & 0x3333;

    // Separate bits into groups of 8.
    // Move pairs up by 1 slot.
    x = (x | (x << 1)) & 0x5555;

    // Step 2: Duplicate bits.
    // We currently have `b_i` at position `2*i`.
    // We want `b_i` at `2*i` and `2*i + 1`.
    // If we have `...b...0...`, shifting left gives `...0...b...`.
    // ORing them combines `...b...b...`.
    x | (x << 1)
}
