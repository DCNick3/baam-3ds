#[inline]
pub const fn unpack_565(packed: u16) -> (u8, u8, u8) {
    let (r, g, b) = unpack_565_u16(packed);

    (r as u8, g as u8, b as u8)
}

#[inline]
pub const fn unpack_565_u16(packed: u16) -> (u16, u16, u16) {
    (
        packed >> 11 & 0b11111,
        packed >> 5 & 0b111111,
        packed & 0b11111,
    )
}

#[inline]
pub const fn pack_565_u16((r5, g6, b5): (u16, u16, u16)) -> u16 {
    debug_assert!(r5 & 0b11111 == r5, "r5 channel too wide");
    debug_assert!(g6 & 0b111111 == g6, "g6 channel too wide");
    debug_assert!(b5 & 0b11111 == b5, "b5 channel too wide");

    (r5 << 11) | (g6 << 5) | b5
}

#[inline]
pub const fn pack_565((r5, g6, b5): (u8, u8, u8)) -> u16 {
    pack_565_u16((r5 as u16, g6 as u16, b5 as u16))
}

pub const fn blend_rgb565(dest: u16, src: u16, alpha: u8) -> u16 {
    let (dr, dg, db) = unpack_565(dest);
    let (sr, sg, sb) = unpack_565(src);

    let r = (((255 - alpha as u16) * dr as u16 + alpha as u16 * sr as u16) / 255) as u8;
    let g = (((255 - alpha as u16) * dg as u16 + alpha as u16 * sg as u16) / 255) as u8;
    let b = (((255 - alpha as u16) * db as u16 + alpha as u16 * sb as u16) / 255) as u8;

    pack_565((r, g, b))
}
