use egui::{Color32, Visuals};
use ququ_core::{ConnectionProfile, EnvironmentTier};

#[derive(Debug, Clone)]
pub struct AppTheme {
    pub dark_mode: bool,
    pub editor_bg: Color32,
    pub editor_gutter_bg: Color32,
    pub editor_gutter_fg: Color32,
    pub keyword: Color32,
    pub string_lit: Color32,
    pub number_lit: Color32,
    pub comment: Color32,
    pub bind_var: Color32,
    pub identifier: Color32,
    pub operator: Color32,
    pub null_fg: Color32,
    pub log_timestamp: Color32,
    pub log_error: Color32,
    pub grid_stripe: Color32,
    pub grid_selected_bg: Color32,
    pub splitter_bg: Color32,
}

impl AppTheme {
    pub fn light() -> Self {
        Self {
            dark_mode: false,
            editor_bg: Color32::WHITE,
            editor_gutter_bg: Color32::from_rgb(245, 246, 248),
            editor_gutter_fg: Color32::from_rgb(120, 130, 145),
            keyword: Color32::from_rgb(0, 64, 196),
            string_lit: Color32::from_rgb(163, 21, 21),
            number_lit: Color32::from_rgb(9, 134, 88),
            comment: Color32::from_rgb(0, 128, 0),
            bind_var: Color32::from_rgb(152, 0, 160),
            identifier: Color32::from_rgb(24, 24, 24),
            operator: Color32::from_rgb(80, 80, 80),
            null_fg: Color32::from_rgb(145, 150, 160),
            log_timestamp: Color32::from_rgb(0, 128, 0),
            log_error: Color32::from_rgb(200, 20, 30),
            grid_stripe: Color32::from_rgb(248, 250, 252),
            grid_selected_bg: Color32::from_rgb(214, 234, 255),
            splitter_bg: Color32::from_rgb(220, 224, 230),
        }
    }

    pub fn dark() -> Self {
        Self {
            dark_mode: true,
            editor_bg: Color32::from_rgb(24, 26, 31),
            editor_gutter_bg: Color32::from_rgb(30, 33, 40),
            editor_gutter_fg: Color32::from_rgb(125, 135, 150),
            keyword: Color32::from_rgb(110, 168, 254),
            string_lit: Color32::from_rgb(235, 155, 120),
            number_lit: Color32::from_rgb(145, 210, 145),
            comment: Color32::from_rgb(106, 153, 85),
            bind_var: Color32::from_rgb(215, 145, 240),
            identifier: Color32::from_rgb(228, 232, 240),
            operator: Color32::from_rgb(180, 190, 200),
            null_fg: Color32::from_rgb(130, 136, 148),
            log_timestamp: Color32::from_rgb(110, 190, 110),
            log_error: Color32::from_rgb(255, 115, 115),
            grid_stripe: Color32::from_rgb(32, 35, 42),
            grid_selected_bg: Color32::from_rgb(44, 78, 122),
            splitter_bg: Color32::from_rgb(50, 54, 64),
        }
    }

    pub fn for_mode(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    pub fn apply_to_ctx(&self, ctx: &egui::Context) {
        let mut visuals = if self.dark_mode {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        visuals.override_text_color = if self.dark_mode {
            Some(Color32::from_rgb(230, 233, 240))
        } else {
            Some(Color32::from_rgb(24, 24, 24))
        };
        ctx.set_visuals(visuals);
    }
}

pub fn env_color(env: EnvironmentTier) -> Color32 {
    match env {
        EnvironmentTier::Local => Color32::from_rgb(45, 164, 78),
        EnvironmentTier::Development => Color32::from_rgb(47, 129, 247),
        EnvironmentTier::Staging => Color32::from_rgb(191, 135, 0),
        EnvironmentTier::Production => Color32::from_rgb(209, 36, 47),
    }
}

pub fn parse_hex_color(hex: &str, fallback: Color32) -> Color32 {
    let s = hex.trim().trim_start_matches('#');
    if s.len() != 6 {
        return fallback;
    }
    let Ok(r) = u8::from_str_radix(&s[0..2], 16) else {
        return fallback;
    };
    let Ok(g) = u8::from_str_radix(&s[2..4], 16) else {
        return fallback;
    };
    let Ok(b) = u8::from_str_radix(&s[4..6], 16) else {
        return fallback;
    };
    Color32::from_rgb(r, g, b)
}

pub fn profile_accent_color(profile: &ConnectionProfile) -> Color32 {
    parse_hex_color(&profile.accent_color, env_color(profile.env))
}
