//! QuQuSQL の egui ベース GUI コンポーネント群を提供するクレート。
//!
//! DB ドライバには直接依存せず、`ququ-core` と `ququ-sql` のみを利用して
//! 高速な SQL エディタ・仮想スクロールグリッド・オブジェクトブラウザ・安全確認モーダルを描画します。

#![warn(missing_docs)]

pub mod browser;
pub mod dialogs;
pub mod editor;
pub mod grid;
pub mod layout;
pub mod state;
pub mod theme;

pub use layout::render_main_ui;
pub use state::{AppAction, UiState};
pub use theme::AppTheme;
