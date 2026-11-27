//! 接続プロファイル・アプリケーション設定・ウィンドウ状態・保存パスの管理モジュール。

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 接続先の環境区分。視覚識別および安全ガード（確認ダイアログ・Read-Only 初期接続）と連動する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EnvironmentTier {
    /// ローカル環境（検証用 DB 等）
    #[default]
    Local,
    /// 開発環境
    Development,
    /// 検証環境（ステージング）
    Staging,
    /// 本番環境（誤操作防止ガードが最大レベルで有効になる）
    Production,
}

impl EnvironmentTier {
    /// すべての環境区分の一覧を返す。
    pub fn all() -> &'static [Self] {
        &[
            Self::Local,
            Self::Development,
            Self::Staging,
            Self::Production,
        ]
    }

    /// UI 表示用の日本語ラベルを返す。
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "ローカル",
            Self::Development => "開発",
            Self::Staging => "検証",
            Self::Production => "本番",
        }
    }

    /// タイトルバーやバッジに表示する識別テキストを返す。
    pub fn badge_text(self) -> &'static str {
        match self {
            Self::Local => "[ローカル]",
            Self::Development => "[開発]",
            Self::Staging => "[検証]",
            Self::Production => "[⚠ 本番]",
        }
    }

    /// 環境区分に応じた推奨アクセントカラー（HEX 文字列）を返す。
    pub fn default_accent_hex(self) -> &'static str {
        match self {
            Self::Local => "#2DA44E",
            Self::Development => "#2F81F7",
            Self::Staging => "#BF8700",
            Self::Production => "#D1242F",
        }
    }

    /// 環境区分に応じた初期読み取り専用フラグの推奨値を返す（本番は `true`）。
    pub fn default_read_only(self) -> bool {
        matches!(self, Self::Production)
    }

    /// コミット時の確認ダイアログ表示の初期値を返す。
    pub fn default_confirm_on_commit(self) -> bool {
        matches!(self, Self::Staging | Self::Production)
    }

    /// DML / DDL 実行時の確認ダイアログ表示の初期値を返す。
    pub fn default_confirm_on_dml_ddl(self) -> bool {
        matches!(self, Self::Staging | Self::Production)
    }

    /// 本番環境かどうかを判定する。
    pub fn is_production(self) -> bool {
        matches!(self, Self::Production)
    }
}

/// 対応している RDBMS の種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DbType {
    /// ローカル検証用 SQLite
    #[default]
    Sqlite,
    /// Pure Rust ドライバ経由の Oracle Database
    Oracle,
}

impl DbType {
    /// UI 表示用の RDBMS 名を返す。
    pub fn label(self) -> &'static str {
        match self {
            Self::Sqlite => "SQLite",
            Self::Oracle => "Oracle",
        }
    }
}

/// Oracle への接続方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OracleConnectMode {
    /// ホスト・ポート・サービス名を直接指定する接続方式
    #[default]
    Direct,
    /// `tnsnames.ora` のエイリアス名で接続する方式
    Tns,
}

/// 接続プロファイル。
///
/// セキュリティ方針によりパスワードは絶対に保持・ディスク保存しない。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfile {
    /// プロファイル一意 ID
    pub id: String,
    /// 表示名
    pub name: String,
    /// 環境区分（本番・検証・開発・ローカル）
    #[serde(default)]
    pub env: EnvironmentTier,
    /// RDBMS 種別
    #[serde(default)]
    pub db_type: DbType,
    /// Oracle 接続方式
    #[serde(default)]
    pub connect_mode: OracleConnectMode,
    /// Oracle ホスト名
    #[serde(default)]
    pub host: String,
    /// Oracle ポート番号
    #[serde(default = "default_oracle_port")]
    pub port: u16,
    /// Oracle サービス名
    #[serde(default)]
    pub service_name: String,
    /// TNS エイリアス名
    #[serde(default)]
    pub tns_name: String,
    /// SQLite データベースファイルパス（`:memory:` も可）
    #[serde(default)]
    pub file_path: String,
    /// 接続ユーザー名
    #[serde(default)]
    pub username: String,
    /// 初期接続を強制読み取り専用にするか
    #[serde(default)]
    pub read_only: bool,
    /// UI アクセントカラー（HEX 形式）
    #[serde(default)]
    pub accent_color: String,
    /// コミット実行前に確認ダイアログを出すか
    #[serde(default)]
    pub confirm_on_commit: bool,
    /// DML / DDL 実行前に確認ダイアログを出すか
    #[serde(default)]
    pub confirm_on_dml_ddl: bool,
}

fn default_oracle_port() -> u16 {
    1521
}

impl ConnectionProfile {
    /// 環境区分と DB 種別に応じたデフォルト値を持つ新規プロファイルを生成する。
    pub fn new_template(env: EnvironmentTier, db_type: DbType) -> Self {
        Self {
            id: format!(
                "conn-{}",
                chrono::Local::now().format("%Y%m%d%H%M%S%3f")
            ),
            name: match db_type {
                DbType::Sqlite => "新規SQLite接続".to_string(),
                DbType::Oracle => "新規Oracle接続".to_string(),
            },
            env,
            db_type,
            connect_mode: OracleConnectMode::Direct,
            host: "127.0.0.1".to_string(),
            port: 1521,
            service_name: "FREEPDB1".to_string(),
            tns_name: String::new(),
            file_path: String::new(),
            username: String::new(),
            read_only: env.default_read_only(),
            accent_color: env.default_accent_hex().to_string(),
            confirm_on_commit: env.default_confirm_on_commit(),
            confirm_on_dml_ddl: env.default_confirm_on_dml_ddl(),
        }
    }

    /// 指定した環境区分の推奨プリセット（色・安全ガード）をプロファイルへ適用する。
    pub fn apply_env_presets(&mut self, env: EnvironmentTier) {
        self.env = env;
        self.accent_color = env.default_accent_hex().to_string();
        self.read_only = env.default_read_only();
        self.confirm_on_commit = env.default_confirm_on_commit();
        self.confirm_on_dml_ddl = env.default_confirm_on_dml_ddl();
    }

    /// タイトルバーやステータスバーに表示する接続先識別文字列を生成する。
    pub fn display_target(&self) -> String {
        match self.db_type {
            DbType::Sqlite => {
                if self.file_path.trim().is_empty() || self.file_path == ":memory:" {
                    ":memory:".to_string()
                } else {
                    Path::new(&self.file_path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&self.file_path)
                        .to_string()
                }
            }
            DbType::Oracle => {
                let db = match self.connect_mode {
                    OracleConnectMode::Direct => {
                        if self.service_name.is_empty() {
                            format!("{}:{}", self.host, self.port)
                        } else {
                            self.service_name.clone()
                        }
                    }
                    OracleConnectMode::Tns => self.tns_name.clone(),
                };
                if self.username.is_empty() {
                    db
                } else {
                    format!("{}@{}", self.username, db)
                }
            }
        }
    }
}

/// 接続時のみメモリ上で扱う認証情報（`Drop` 時に内容をゼロクリアする）。
#[derive(Debug, Clone, Default)]
pub struct ConnectCredentials {
    /// 都度入力されたパスワード
    pub password: Option<String>,
}

impl Drop for ConnectCredentials {
    fn drop(&mut self) {
        if let Some(ref mut pw) = self.password {
            // 安全のためメモリ上のパスワードを 0 で上書きしてから解放する
            unsafe {
                for b in pw.as_bytes_mut() {
                    std::ptr::write_volatile(b, 0);
                }
            }
            pw.clear();
        }
    }
}

/// `connections.toml` のルート構造。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionsFile {
    /// 登録済み接続プロファイル一覧
    #[serde(default)]
    pub connections: Vec<ConnectionProfile>,
}

impl Default for ConnectionsFile {
    fn default() -> Self {
        let mut memory_sqlite =
            ConnectionProfile::new_template(EnvironmentTier::Local, DbType::Sqlite);
        memory_sqlite.id = "default-sqlite-memory".to_string();
        memory_sqlite.name = "インメモリ検証 (SQLite)".to_string();
        memory_sqlite.file_path = ":memory:".to_string();

        Self {
            connections: vec![memory_sqlite],
        }
    }
}

/// `config.toml` に保存するアプリケーション全体設定。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// 新規タブ作成時のデフォルト文字コード
    pub default_encoding: String,
    /// デフォルト改行コード
    pub default_line_ending: String,
    /// ダークテーマ有効フラグ
    pub dark_mode: bool,
    /// エディタのフォントサイズ
    pub font_size: f32,
    /// 大量レコード取得時の初期フェッチ上限行数（デフォルト 1,000 行）
    pub default_fetch_limit: usize,
    /// 日時データのデフォルト表示フォーマット
    pub date_format: String,
    /// `tnsnames.ora` を探索するディレクトリ／ファイルの手動指定パス
    pub tns_admin_path: String,
    /// `WHERE` 句なしの UPDATE / DELETE 実行時に確認ダイアログを出すか
    pub confirm_where_less_dml: bool,
    /// 接続一覧で本番環境セクションをデフォルトで折りたたむか
    pub collapse_production_connections: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_encoding: "utf-8".to_string(),
            default_line_ending: "crlf".to_string(),
            dark_mode: false,
            font_size: 13.5,
            default_fetch_limit: 1000,
            date_format: "YYYY-MM-DD HH:MM:SS".to_string(),
            tns_admin_path: String::new(),
            confirm_where_less_dml: true,
            collapse_production_connections: true,
        }
    }
}

/// ウィンドウサイズと各スプリットペインの分割比率。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    /// ウィンドウ幅
    pub width: f32,
    /// ウィンドウ高さ
    pub height: f32,
    /// 左右スプリット比率（左ペインの幅割合）
    pub left_panel_ratio: f32,
    /// 左エリア上下スプリット比率（エディタ高さ割合）
    pub left_split_ratio: f32,
    /// 右エリア上下スプリット比率（オブジェクト一覧高さ割合）
    pub right_split_ratio: f32,
    /// 右側オブジェクトブラウザの表示状態
    pub show_browser: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1280.0,
            height: 820.0,
            left_panel_ratio: 0.68,
            left_split_ratio: 0.56,
            right_split_ratio: 0.55,
            show_browser: true,
        }
    }
}

/// Windows 標準ディレクトリ（`%APPDATA%` / `%LOCALAPPDATA%`）配下の設定・キャッシュ保存パス。
#[derive(Debug, Clone)]
pub struct StoragePaths {
    /// 設定ファイル保存ディレクトリ
    pub config_dir: PathBuf,
    /// キャッシュファイル保存ディレクトリ
    pub cache_dir: PathBuf,
}

impl StoragePaths {
    /// OS 標準のアプリケーションディレクトリを解決する。
    pub fn resolve() -> Option<Self> {
        let dirs = ProjectDirs::from("", "", "QuQuSQL")?;
        Some(Self {
            config_dir: dirs.config_dir().to_path_buf(),
            cache_dir: dirs.cache_dir().to_path_buf(),
        })
    }

    /// `config.toml` のパスを返す。
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// `connections.toml` のパスを返す。
    pub fn connections_file(&self) -> PathBuf {
        self.config_dir.join("connections.toml")
    }

    /// `window_state.json` のパスを返す。
    pub fn window_state_file(&self) -> PathBuf {
        self.cache_dir.join("window_state.json")
    }

    /// `config.toml` を読み込む（存在しない・不正な場合はデフォルト値）。
    pub fn load_config(&self) -> AppConfig {
        fs::read_to_string(self.config_file())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// `config.toml` へ設定を保存する。
    pub fn save_config(&self, config: &AppConfig) -> std::io::Result<()> {
        fs::create_dir_all(&self.config_dir)?;
        let content = toml::to_string_pretty(config)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(self.config_file(), content)
    }

    /// `connections.toml` を読み込む。
    pub fn load_connections(&self) -> ConnectionsFile {
        fs::read_to_string(self.connections_file())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// `connections.toml` へ接続プロファイル一覧を保存する。
    pub fn save_connections(&self, data: &ConnectionsFile) -> std::io::Result<()> {
        fs::create_dir_all(&self.config_dir)?;
        let content = toml::to_string_pretty(data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(self.connections_file(), content)
    }

    /// `window_state.json` を読み込む。
    pub fn load_window_state(&self) -> WindowState {
        fs::read_to_string(self.window_state_file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// `window_state.json` へウィンドウ状態を保存する。
    pub fn save_window_state(&self, state: &WindowState) -> std::io::Result<()> {
        fs::create_dir_all(&self.cache_dir)?;
        let content = serde_json::to_string_pretty(state)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(self.window_state_file(), content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_preset_enables_strict_safety_guards() {
        let p = ConnectionProfile::new_template(EnvironmentTier::Production, DbType::Oracle);
        assert!(p.read_only);
        assert!(p.confirm_on_commit);
        assert!(p.confirm_on_dml_ddl);
        assert_eq!(p.accent_color, "#D1242F");
    }

    #[test]
    fn connection_profile_never_serializes_password() {
        let profile = ConnectionProfile::new_template(EnvironmentTier::Development, DbType::Oracle);
        let toml_text = toml::to_string_pretty(&profile).unwrap();
        assert!(!toml_text.contains("password"));
    }

    #[test]
    fn apply_env_presets_and_display_target() {
        let mut prof = ConnectionProfile::new_template(EnvironmentTier::Local, DbType::Oracle);
        assert!(!prof.read_only);
        prof.apply_env_presets(EnvironmentTier::Production);
        assert!(prof.read_only);
        assert!(prof.confirm_on_commit);
        assert!(prof.confirm_on_dml_ddl);
        assert_eq!(prof.accent_color, "#D1242F");

        prof.username = "SCOTT".to_string();
        prof.service_name = "ORCL".to_string();
        assert_eq!(prof.display_target(), "SCOTT@ORCL");

        prof.connect_mode = OracleConnectMode::Tns;
        prof.tns_name = "PROD_TNS".to_string();
        assert_eq!(prof.display_target(), "SCOTT@PROD_TNS");

        let mut sqlite = ConnectionProfile::new_template(EnvironmentTier::Local, DbType::Sqlite);
        sqlite.file_path = "C:/work/test.db".to_string();
        assert_eq!(sqlite.display_target(), "test.db");
    }
}
