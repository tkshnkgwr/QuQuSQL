//! アプリ全体の配色テーマ（ライト / ダーク）および環境区分カラー定義モジュール。

use egui::{Color32, Visuals};
use ququ_core::{ConnectionProfile, EnvironmentTier};

/// SQL シンタックスハイライト、エディタ、グリッド、ログで使用する配色セット。
#[derive(Debug, Clone)]
pub struct AppTheme {
    /// ダークテーマかどうか
    pub dark_mode: bool,
    /// エディタ背景色
    pub editor_bg: Color32,
    /// 行番号ガター背景色
    pub editor_gutter_bg: Color32,
    /// 行番号文字色
    pub editor_gutter_fg: Color32,
    /// SQL キーワード文字色
    pub keyword: Color32,
    /// 文字列リテラル文字色
    pub string_lit: Color32,
    /// 数値リテラル文字色
    pub number_lit: Color32,
    /// コメント文字色
    pub comment: Color32,
    /// バインド変数文字色
    pub bind_var: Color32,
    /// 通常識別子文字色
    pub identifier: Color32,
    /// 演算子・記号文字色
    pub operator: Color32,
    /// `<NULL>` や補助テキストの文字色
    pub null_fg: Color32,
    /// ログのタイムスタンプ文字色
    pub log_timestamp: Color32,
    /// ログのエラーメッセージ文字色
    pub log_error: Color32,
    /// グリッドの交互行背景色
    pub grid_stripe: Color32,
    /// グリッド・一覧の選択セル／行背景色
    pub grid_selected_bg: Color32,
    /// スプリッタ（境界ドラッグバー）背景色
    pub splitter_bg: Color32,
}

impl AppTheme {
    /// OSqlEdit 風の視認性重視ライトテーマを返す。
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

    /// 夜間作業や暗色環境向けのダークテーマを返す。
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

    /// ダークモードフラグに応じたテーマを返す。
    pub fn for_mode(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    /// `egui::Context` のビジュアル設定へテーマを適用する。
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

/// 環境区分に対応する識別カラーを返す。
pub fn env_color(env: EnvironmentTier) -> Color32 {
    match env {
        EnvironmentTier::Local => Color32::from_rgb(45, 164, 78),
        EnvironmentTier::Development => Color32::from_rgb(47, 129, 247),
        EnvironmentTier::Staging => Color32::from_rgb(191, 135, 0),
        EnvironmentTier::Production => Color32::from_rgb(209, 36, 47),
    }
}

/// `#RRGGBB` 形式のカラー文字列を `Color32` へ変換する（不正な場合は `fallback`）。
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

/// プロファイルに設定されたアクセントカラー（未設定・不正時は環境デフォルト色）を返す。
pub fn profile_accent_color(profile: &ConnectionProfile) -> Color32 {
    parse_hex_color(&profile.accent_color, env_color(profile.env))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_color_with_fallback() {
        assert_eq!(
            parse_hex_color("#D1242F", Color32::BLACK),
            Color32::from_rgb(209, 36, 47)
        );
        assert_eq!(parse_hex_color("invalid", Color32::WHITE), Color32::WHITE);
    }
}
