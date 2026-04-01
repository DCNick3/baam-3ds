use crate::texture_data::{
    PAL_GLOW_MASK, PAL_LIGHT_GREEN_OFF, PAL_LIGHT_GREEN_ON, PAL_LIGHT_RED_OFF, PAL_LIGHT_RED_ON,
    PAL_LIGHT_YELLOW_OFF, PAL_LIGHT_YELLOW_ON, TEX_GLOW_MASK, TEX_LIGHT,
};
use imgref::ImgRefMut;

#[derive(Copy, Clone)]
pub enum LightColor {
    Red,
    Green,
    Yellow,
}

fn get_palette(color: LightColor, state: bool) -> &'static [u16; 16] {
    match (color, state) {
        (LightColor::Red, true) => &PAL_LIGHT_RED_ON,
        (LightColor::Red, false) => &PAL_LIGHT_RED_OFF,
        (LightColor::Green, true) => &PAL_LIGHT_GREEN_ON,
        (LightColor::Green, false) => &PAL_LIGHT_GREEN_OFF,
        (LightColor::Yellow, true) => &PAL_LIGHT_YELLOW_ON,
        (LightColor::Yellow, false) => &PAL_LIGHT_YELLOW_OFF,
    }
}

pub struct LightState {
    luminosity: u8,
}

impl LightState {
    pub fn new() -> Self {
        Self { luminosity: 0 }
    }

    pub fn draw(&mut self, mut dest: ImgRefMut<u16>, color: LightColor, state: bool) {
        let palette = get_palette(color, state);

        TEX_LIGHT.draw_paletted_transparent(dest.sub_image_mut(4, 4, 16, 16), palette);

        if state {
            let active_color = palette[1];
            // apply adjustments to the mask palette to make glows of different colors look similar-ish
            let mask = match color {
                LightColor::Red => PAL_GLOW_MASK.map(|v| (v as u16 * 100 / 100) as u8),
                LightColor::Green => PAL_GLOW_MASK.map(|v| (v as u16 * 120 / 100) as u8),
                LightColor::Yellow => PAL_GLOW_MASK.map(|v| (v as u16 * 140 / 100) as u8),
            };

            TEX_GLOW_MASK.draw_blend(dest, &mask, active_color);
        }
    }
}
