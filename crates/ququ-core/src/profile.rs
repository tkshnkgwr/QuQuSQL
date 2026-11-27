use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 接続先の環境区分。視覚識別および安全ガード（確認ダイアログ・Read-Only初期接続）と連動する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EnvironmentTier {
    #[default]
    Local,
    Development,
    Staging,
    Production,
}

impl EnvironmentTier {
    pub fn all() -> &'static [Self] {
        &[
            Self::Local,
            Self::Development,
            Self::Staging,
            Self::Production,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "ローカル",
            Self::Development => "開発",
            Self::Staging => "検証",
            Self::Production => "本番",
        }
    }

    pub fn badge_text(self) -> &'static str {
        match self {
            Self::Local => "[ローカル]",
            Self::Development => "[開発]",
            Self::Staging => "[検証]",
            Self::Production => "[⚠ 本番]",
        }
    }

    pub fn default_accent_hex(self) -> &'static str {
        match self {
            Self::Local => "#2DA44E",
            Self::Development => "#2F81F7",
            Self::Staging => "#BF8700",
            Self::Production => "#D1242F",
        }
    }

    pub fn default_read_only(self) -> bool {
        matches!(self, Self::Production)
    }

    pub fn default_confirm_on_commit(self) -> bool {
        matches!(self, Self::Staging | Self::Production)
    }

    pub fn default_confirm_on_dml_ddl(self) -> bool {
        matches!(self, Self::Staging | Self::Production)
    }

    pub fn is_production(self) -> bool {
        matches!(self, Self::Production)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DbType {
    #[default]
    Sqlite,
    Oracle,
}

impl DbType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sqlite => "SQLite",
            Self::Oracle => "Oracle",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OracleConnectMode {
    #[default]
    Direct,
    Tns,
}

/// 接続プロファイル（セキュリティ方針によりパスワードは絶対に保持・保存しない）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub env: EnvironmentTier,
    #[serde(default)]
    pub db_type: DbType,
    #[serde(default)]
    pub connect_mode: OracleConnectMode,
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_oracle_port")]
    pub port: u16,
    #[serde(default)]
    pub service_name: String,
    #[serde(default)]
    pub tns_name: String,
    #[serde(default)]
    pub file_path: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub accent_color: String,
    #[serde(default)]
    pub confirm_on_commit: bool,
    #[serde(default)]
    pub confirm_on_dml_ddl: bool,
}

fn default_oracle_port() -> u16 {
    1521
}

impl ConnectionProfile {
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

    pub fn apply_env_presets(&mut self, env: EnvironmentTier) {
        self.env = env;
        self.accent_color = env.default_accent_hex().to_string();
        self.read_only = env.default_read_only();
        self.confirm_on_commit = env.default_confirm_on_commit();
        self.confirm_on_dml_ddl = env.default_confirm_on_dml_ddl();
    }

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

/// 接続時のみメモリ上で扱う認証情報（Drop 時に内容をゼロクリアする）。
#[derive(Debug, Clone, Default)]
pub struct ConnectCredentials {
    pub password: Option<String>,
}

impl Drop for ConnectCredentials {
    fn drop(&mut self) {
        if let Some(ref mut pw) = self.password {
            // 安全のためメモリ上のパスワードを 0 で上書きしてから解放
            unsafe {
                for b in pw.as_bytes_mut() {
                    std::ptr::write_volatile(b, 0);
                }
            }
            pw.clear();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionsFile {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub default_encoding: String,
    pub default_line_ending: String,
    pub dark_mode: bool,
    pub font_size: f32,
    pub default_fetch_limit: usize,
    pub date_format: String,
    pub tns_admin_path: String,
    pub confirm_where_less_dml: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub width: f32,
    pub height: f32,
    pub left_panel_ratio: f32,
    pub left_split_ratio: f32,
    pub right_split_ratio: f32,
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

#[derive(Debug, Clone)]
pub struct StoragePaths {
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl StoragePaths {
    pub fn resolve() -> Option<Self> {
        let dirs = ProjectDirs::from("", "", "QuQuSQL")?;
        Some(Self {
            config_dir: dirs.config_dir().to_path_buf(),
            cache_dir: dirs.cache_dir().to_path_buf(),
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn connections_file(&self) -> PathBuf {
        self.config_dir.join("connections.toml")
    }

    pub fn window_state_file(&self) -> PathBuf {
        self.cache_dir.join("window_state.json")
    }

    pub fn load_config(&self) -> AppConfig {
        fs::read_to_string(self.config_file())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save_config(&self, config: &AppConfig) -> std::io::Result<()> {
        fs::create_dir_all(&self.config_dir)?;
        let content = toml::to_string_pretty(config)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(self.config_file(), content)
    }

    pub fn load_connections(&self) -> ConnectionsFile {
        fs::read_to_string(self.connections_file())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save_connections(&self, data: &ConnectionsFile) -> std::io::Result<()> {
        fs::create_dir_all(&self.config_dir)?;
        let content = toml::to_string_pretty(data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(self.connections_file(), content)
    }

    pub fn load_window_state(&self) -> WindowState {
        fs::read_to_string(self.window_state_file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

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
}
