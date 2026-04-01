use crate::blend;
use imgref::ImgRefMut;

#[derive(Copy, Clone, Default)]
pub struct Transform {
    pub flip_x: bool,
    pub flip_y: bool,
}

pub struct TexNData<'a, const N: u8> {
    pub width: usize,
    pub height: usize,
    pub data: &'a [u8],
}

impl<const N: u8> TexNData<'_, N> {
    #[inline]
    fn draw_impl(&self, mut dest: ImgRefMut<u16>, transform: Transform, f: impl Fn(&mut u16, u8)) {
        let (src_w, src_h) = (self.width, self.height);

        for (y, row) in (0..src_h).zip(dest.rows_mut()) {
            for (x, pixel) in (0..src_w).zip(row) {
                let y = if transform.flip_y { src_h - y - 1 } else { y };
                let x = if transform.flip_x { src_w - x - 1 } else { x };
                let value = unsafe { self.get_value_at_unchecked(x, y) };
                f(pixel, value)
            }
        }
    }

    #[inline]
    pub unsafe fn get_value_at_unchecked(&self, x: usize, y: usize) -> u8 {
        const {
            assert!(N == 1 || N == 2 || N == 4);
        }
        let pixels_per_byte: usize = (8 / N) as usize;
        let pixel_mask: u8 = (1 << N) - 1;

        let i = y * self.width + x;

        let byte_index = i / pixels_per_byte;
        let index_in_byte = (i % pixels_per_byte) as u8;

        // FIXME: the bounds check this generates can be wasteful
        let &packed = self.data.get_unchecked(byte_index);
        let value = (packed >> (index_in_byte * N)) & pixel_mask;

        value
    }
}

pub type Tex1Data<'a> = TexNData<'a, 1>;
pub type Tex2Data<'a> = TexNData<'a, 2>;
pub type Tex4Data<'a> = TexNData<'a, 4>;

impl Tex1Data<'_> {
    pub fn draw_with_shadow(&self, mut dest: ImgRefMut<u16>, foreground: u16, shadow: u16) {
        let dest_height = dest.height();
        let dest_stride = dest.stride();
        let buf = dest.buf_mut();

        for y in 0..self.height {
            for x in 0..self.width {
                let i = y * self.width + x;

                let byte_index = i / 8;
                let index_in_byte = (i % 8) as u8;

                // FIXME: the bounds check this generates can be wasteful
                let packed = self.data[byte_index];
                let value = (packed >> index_in_byte) & 1;

                if value != 0 {
                    buf[y * dest_stride + x + 1] = foreground;
                    if y + 1 < dest_height {
                        buf[(y + 1) * dest_stride + x] = shadow;
                    }
                }
            }
        }
    }

    pub fn draw_transparent(&self, dest: ImgRefMut<u16>, color: u16) {
        self.draw_impl(dest, Transform::default(), |pixel, index| {
            if index == 0 {
                return;
            }

            *pixel = color;
        })
    }
    pub fn draw_opaque(&self, dest: ImgRefMut<u16>, palette: &[u16; 2]) {
        self.draw_impl(dest, Transform::default(), |pixel, index| {
            *pixel = palette[index as usize];
        })
    }
}

impl Tex2Data<'_> {
    pub fn draw_paletted_transparent_with_transform(
        &self,
        dest: ImgRefMut<u16>,
        palette: &[u16; 4],
        transform: Transform,
    ) {
        self.draw_impl(dest, transform, |pixel, index| {
            if index == 0 {
                return;
            }

            *pixel = palette[index as usize];
        })
    }
    pub fn draw_tinted(
        &self,
        dest: ImgRefMut<u16>,
        alpha_palette: &[u8; 4],
        bg_color: u16,
        fg_color: u16,
    ) {
        self.draw_impl(dest, Transform::default(), |pixel, index| {
            let alpha = alpha_palette[index as usize];
            *pixel = blend::blend_rgb565(bg_color, fg_color, alpha);
        });
    }
    pub fn draw_blend(&self, dest: ImgRefMut<u16>, alpha_palette: &[u8; 4], color: u16) {
        self.draw_impl(dest, Transform::default(), |pixel, index| {
            let alpha = alpha_palette[index as usize];
            *pixel = blend::blend_rgb565(*pixel, color, alpha);
        });
    }
}

impl Tex4Data<'_> {
    pub fn draw_paletted_transparent(&self, dest: ImgRefMut<u16>, palette: &[u16; 16]) {
        self.draw_impl(dest, Transform::default(), |pixel, index| {
            if index == 0 {
                return;
            }

            *pixel = palette[index as usize];
        })
    }
    pub fn draw_paletted_opaque(&self, dest: ImgRefMut<u16>, palette: &[u16; 16]) {
        self.draw_impl(dest, Transform::default(), |pixel, index| {
            *pixel = palette[index as usize];
        })
    }
}
