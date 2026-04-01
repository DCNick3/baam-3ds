mod terminal;

use crate::lights::{LightColor, LightState};
use crate::scene::terminal::TerminalState;
use crate::{texture::Transform, texture_data};
use imgref::{ImgExtMut, ImgRefMut};

#[derive(Copy, Clone)]
pub enum BreadcrumbRequest {
    Login(bool),
    Mark(bool),
    Success,
}

pub struct NetworkRequest {
    pub active: bool,
    pub error: bool,
}

pub enum CameraQrTestRequest {
    None,
    Accepted,
    Rejected,
}

pub struct CameraRequest {
    pub active: bool,
    pub scan: CameraQrTestRequest,
}

pub struct SceneRequest {
    pub tick: u32,
    pub breadcrumb_request: BreadcrumbRequest,
    pub network_request: NetworkRequest,
    pub camera_request: CameraRequest,
}

struct LightsState {
    // breadcrumb LEDs
    breadcrumb_login: LightState,
    breadcrumb_mark: LightState,
    breadcrumb_success: LightState,

    // NET panel
    net_act: LightState,
    net_err: LightState,

    // CAM panel
    cam_act: LightState,
    cam_scn: LightState,
}

impl LightsState {
    pub fn new() -> Self {
        Self {
            breadcrumb_login: LightState::new(),
            breadcrumb_mark: LightState::new(),
            breadcrumb_success: LightState::new(),
            net_act: LightState::new(),
            net_err: LightState::new(),
            cam_act: LightState::new(),
            cam_scn: LightState::new(),
        }
    }

    pub fn draw(&mut self, mut dest: ImgRefMut<u16>, request: &SceneRequest) {
        use LightColor::*;

        let (login_color, login_state, mark_color, mark_state, success_state) =
            match request.breadcrumb_request {
                BreadcrumbRequest::Login(true) => (Green, true, Green, false, false),
                BreadcrumbRequest::Login(false) => (Red, true, Green, false, false),
                BreadcrumbRequest::Mark(true) => (Green, true, Green, true, false),
                BreadcrumbRequest::Mark(false) => (Green, true, Red, true, false),
                BreadcrumbRequest::Success => (Green, true, Green, true, true),
            };

        self.breadcrumb_login
            .draw(dest.sub_image_mut(214, 3, 24, 24), login_color, login_state);
        self.breadcrumb_mark
            .draw(dest.sub_image_mut(214, 69, 24, 24), mark_color, mark_state);
        self.breadcrumb_success
            .draw(dest.sub_image_mut(214, 131, 24, 24), Green, success_state);

        // "NET ACT" LED
        self.net_act.draw(
            dest.sub_image_mut(192, 258, 24, 24),
            Yellow,
            request.network_request.active,
        );
        // "NET ERR" LED
        self.net_err.draw(
            dest.sub_image_mut(177, 258, 24, 24),
            Red,
            request.network_request.error,
        );

        // "CAM ACT" LED
        self.cam_act.draw(
            dest.sub_image_mut(141, 258, 24, 24),
            Yellow,
            request.camera_request.active,
        );

        let (cam_scn_color, cam_scn_state) = match request.camera_request.scan {
            CameraQrTestRequest::None => (Green, false),
            CameraQrTestRequest::Accepted => (Green, true),
            CameraQrTestRequest::Rejected => (Red, true),
        };

        // "CAM SCN" LED
        self.cam_scn.draw(
            dest.sub_image_mut(126, 258, 24, 24),
            cam_scn_color,
            cam_scn_state,
        );
    }
}

pub struct SceneState {
    pub terminal: TerminalState,
    lights: LightsState,
}

impl SceneState {
    pub fn new() -> Self {
        Self {
            terminal: TerminalState::new(),
            lights: LightsState::new(),
        }
    }

    pub fn draw(&mut self, mut dest: ImgRefMut<u16>, request: &SceneRequest) {
        texture_data::TEX_DECOR.draw_paletted_opaque(dest.as_mut(), &texture_data::PAL_DECOR);
        texture_data::TEX_TEXT_BREADCRUMBS.draw_with_shadow(
            dest.sub_image_mut(220, 24, 12, 177),
            0xffff,
            0x7bcf,
        );
        texture_data::TEX_TEXT_PANEL.draw_with_shadow(
            dest.sub_image_mut(126, 258, 97, 48),
            0xffff,
            0x7bcf,
        );

        // display contents
        self.terminal.draw(dest.as_mut(), request);

        // display overlay (inner rounded corners)
        // top left
        texture_data::TEX_SCREEN_OVERLAY.draw_paletted_transparent_with_transform(
            dest.sub_image_mut(206, 12, 3, 4),
            &texture_data::PAL_SCREEN_OVERLAY_REGULAR,
            Transform {
                flip_x: false,
                flip_y: false,
            },
        );
        // bottom left
        texture_data::TEX_SCREEN_OVERLAY.draw_paletted_transparent_with_transform(
            dest.sub_image_mut(13, 12, 3, 4),
            &texture_data::PAL_SCREEN_OVERLAY_REGULAR,
            Transform {
                flip_x: true,
                flip_y: false,
            },
        );
        // top right
        texture_data::TEX_SCREEN_OVERLAY.draw_paletted_transparent_with_transform(
            dest.sub_image_mut(206, 232, 3, 4),
            &texture_data::PAL_SCREEN_OVERLAY_REGULAR,
            Transform {
                flip_x: false,
                flip_y: true,
            },
        );
        // bottom right
        texture_data::TEX_SCREEN_OVERLAY.draw_paletted_transparent_with_transform(
            dest.sub_image_mut(13, 232, 3, 4),
            &texture_data::PAL_SCREEN_OVERLAY_BOT_RIGHT,
            Transform {
                flip_x: true,
                flip_y: true,
            },
        );

        // lights
        self.lights.draw(dest.as_mut(), request);
    }
}
