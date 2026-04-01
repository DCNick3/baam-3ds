use crate::blend;
use imgref::ImgRefMut;

pub fn scanlines(mut image: ImgRefMut<u16>, scanline_alpha: u8) {
    for (row_number, row) in (0..).zip(image.rows_mut()) {
        let alpha = if row_number % 3 < 2 {
            255
        } else {
            scanline_alpha
        };

        for pixel in row {
            *pixel = blend::blend_rgb565(0x0000, *pixel, alpha);
        }
    }
}

#[inline]
pub const fn lineblur_kernel(indices: [u8; 3], alpha_palette: &[u8], fg: u16, bg: u16) -> u16 {
    let alpha = [
        alpha_palette[indices[0] as usize],
        alpha_palette[indices[1] as usize],
        alpha_palette[indices[2] as usize],
    ];
    let colors = [
        blend::blend_rgb565(bg, fg, alpha[0]),
        blend::blend_rgb565(bg, fg, alpha[1]),
        blend::blend_rgb565(bg, fg, alpha[2]),
    ];

    let (lr, lg, lb) = blend::unpack_565_u16(colors[0]);
    let (cr, cg, cb) = blend::unpack_565_u16(colors[1]);
    let (rr, rg, rb) = blend::unpack_565_u16(colors[2]);

    let (or, og, ob) = (
        (lr + 2 * cr + rr) / 4,
        (lg + 2 * cg + rg) / 4,
        (lb + 2 * cb + rb) / 4,
    );

    if cr + cg + cb < or + og + ob {
        blend::pack_565_u16((or, og, ob))
    } else {
        colors[1]
    }
}

pub fn lineblur(mut image: ImgRefMut<u16>) {
    for row in image.rows_mut() {
        let mut prev = 0x0000;

        for c in 0..row.len() {
            if c > 0 && c < row.len() - 1 {
                let (lr, lg, lb) = blend::unpack_565_u16(prev);
                let (cr, cg, cb) = blend::unpack_565_u16(row[c]);
                let (rr, rg, rb) = blend::unpack_565_u16(row[c + 1]);

                prev = row[c];

                let (or, og, ob) = (
                    (lr + 2 * cr + rr) / 4,
                    (lg + 2 * cg + rg) / 4,
                    (lb + 2 * cb + rb) / 4,
                );

                // only allow increasing the brightness
                if cr + cg + cb < or + og + ob {
                    row[c] = blend::pack_565_u16((or, og, ob));
                }
            } else {
                prev = row[c];
            }
        }
    }
}
