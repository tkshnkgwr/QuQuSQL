use ququ_core::{
    AppConfig, ColumnDetail, ConnectCredentials, ConnectionProfile, EnvironmentTier, ObjectType,
    QueryLogEntry, ResultSet, SchemaObject, StoragePaths, WindowState,
};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SqlTab {
    pub id: usize,
    pub title: String,
    pub file_path: Option<PathBuf>,
    pub text: String,
    pub dirty: bool,
    pub cursor_byte_offset: usize,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub selection_byte_range: Option<(usize, usize)>,
    pub encoding_label: String,
}

impl SqlTab {
    pub fn new(id: usize, title: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            file_path: None,
            text: text.into(),
            dirty: false,
            cursor_byte_offset: 0,
            cursor_line: 1,
            cursor_col: 1,
            selection_byte_range: None,
            encoding_label: "UTF-8, CRLF".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FindReplaceState {
    pub open: bool,
    pub show_replace: bool,
    pub query: String,
    pub replace_with: String,
    pub case_sensitive: bool,
    pub use_regex: bool,
    pub status_msg: String,
    pub pending_select_range: Option<(usize, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Log,
    Grid,
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    ProductionCommit { uncommitted_count: usize },
    ProductionSql {
        sql: String,
        is_ddl: bool,
        has_uncommitted: bool,
        uncommitted_count: usize,
    },
    DmlWithoutWhere { sql: String },
    ImplicitCommitDdl { sql: String, uncommitted_count: usize },
    ExitWithUncommitted { uncommitted_count: usize },
}

#[derive(Debug, Clone)]
pub struct ConfirmModal {
    pub title: String,
    pub message: String,
    pub sql_preview: Option<String>,
    pub is_danger: bool,
    pub confirm_label: String,
    pub action: ConfirmAction,
}

#[derive(Debug, Clone)]
pub struct LoginDialogState {
    pub open: bool,
    pub selected_profile_id: Option<String>,
    pub password_input: String,
    pub editing_profile: Option<ConnectionProfile>,
    pub is_creating_new: bool,
    pub error_msg: Option<String>,
    pub production_expanded: bool,
}

impl Default for LoginDialogState {
    fn default() -> Self {
        Self {
            open: true,
            selected_profile_id: Some("default-sqlite-memory".to_string()),
            password_input: String::new(),
            editing_profile: None,
            is_creating_new: false,
            error_msg: None,
            production_expanded: false,
        }
    }
}

#[derive(Debug)]
pub enum AppAction {
    Connect {
        profile: ConnectionProfile,
        credentials: ConnectCredentials,
        tns_admin_override: Option<String>,
    },
    Disconnect,
    ExecuteSql {
        sql: String,
        allow_write: bool,
    },
    FetchMore {
        sql: String,
        offset: usize,
        limit: usize,
    },
    Commit,
    Rollback,
    LoadOwners,
    LoadObjects {
        owner: String,
        object_type: ObjectType,
    },
    LoadColumns {
        owner: String,
        table_name: String,
    },
    SaveProfiles,
    SaveConfig,
    ConfirmExitClose,
}

pub struct UiState {
    pub config: AppConfig,
    pub window_state: WindowState,
    pub storage: Option<StoragePaths>,
    pub profiles: Vec<ConnectionProfile>,

    pub active_profile: Option<ConnectionProfile>,
    pub db_info: Option<String>,
    pub read_only_session: bool,
    pub uncommitted_count: usize,
    pub busy: bool,

    pub tabs: Vec<SqlTab>,
    pub active_tab: usize,
    pub next_tab_id: usize,
    pub find_replace: FindReplaceState,

    pub bottom_tab: BottomTab,
    pub logs: Vec<QueryLogEntry>,
    pub result: Option<ResultSet>,
    pub selected_cell: Option<(usize, usize)>,

    pub owners: Vec<String>,
    pub selected_owner: String,
    pub selected_object_type: ObjectType,
    pub object_filter: String,
    pub objects: Vec<SchemaObject>,
    pub selected_object_idx: Option<usize>,

    pub column_filter: String,
    pub columns: Vec<ColumnDetail>,

    pub login_dialog: LoginDialogState,
    pub confirm_modal: Option<ConfirmModal>,
    pub pending_exit: bool,
    pub allow_force_exit: bool,
    pub status_notice: Option<String>,
}

impl UiState {
    pub fn new(
        config: AppConfig,
        window_state: WindowState,
        profiles: Vec<ConnectionProfile>,
        storage: Option<StoragePaths>,
    ) -> Self {
        let initial_tab = SqlTab::new(
            1,
            "Query1.sql",
            "-- QuQuSQL へようこそ！\n-- Ctrl+Enter または F5 でカーソル位置の SQL を実行します。\n",
        );

        let default_selected = profiles.first().map(|p| p.id.clone());
        let login_dialog = LoginDialogState {
            open: true,
            selected_profile_id: default_selected,
            ..Default::default()
        };

        Self {
            config,
            window_state,
            storage,
            profiles,
            active_profile: None,
            db_info: None,
            read_only_session: true,
            uncommitted_count: 0,
            busy: false,
            tabs: vec![initial_tab],
            active_tab: 0,
            next_tab_id: 2,
            find_replace: FindReplaceState::default(),
            bottom_tab: BottomTab::Log,
            logs: Vec::new(),
            result: None,
            selected_cell: None,
            owners: Vec::new(),
            selected_owner: String::new(),
            selected_object_type: ObjectType::Table,
            object_filter: String::new(),
            objects: Vec::new(),
            selected_object_idx: None,
            column_filter: String::new(),
            columns: Vec::new(),
            login_dialog,
            confirm_modal: None,
            pending_exit: false,
            allow_force_exit: false,
            status_notice: None,
        }
    }

    pub fn active_tab_ref(&self) -> Option<&SqlTab> {
        self.tabs.get(self.active_tab)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut SqlTab> {
        self.tabs.get_mut(self.active_tab)
    }

    pub fn add_new_tab(&mut self) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = SqlTab::new(id, format!("Query{id}.sql"), "");
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
    }

    pub fn close_tab(&mut self, index: usize) {
        if self.tabs.len() <= 1 || index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
    }

    pub fn is_production(&self) -> bool {
        self.active_profile
            .as_ref()
            .is_some_and(|p| p.env == EnvironmentTier::Production)
    }

    pub fn push_log(&mut self, sql: String, summary: String, is_error: bool, elapsed_ms: Option<u64>) {
        let timestamp = chrono::Local::now().format("%Y/%m/%d %H:%M:%S").to_string();
        self.logs.push(QueryLogEntry {
            timestamp,
            sql,
            summary,
            is_error,
            elapsed_ms,
        });
        if self.logs.len() > 500 {
            let drain = self.logs.len() - 500;
            self.logs.drain(0..drain);
        }
    }

    pub fn window_title(&self) -> String {
        let tab_label = self
            .active_tab_ref()
            .map(|t| {
                if t.dirty {
                    format!("{} *", t.title)
                } else {
                    t.title.clone()
                }
            })
            .unwrap_or_else(|| "Query".to_string());

        match &self.active_profile {
            Some(p) => format!(
                "{} {} ({}) - QuQuSQL - [{}]",
                p.env.badge_text(),
                p.name,
                p.display_target(),
                tab_label
            ),
            None => format!("(未接続) - QuQuSQL - [{tab_label}]"),
        }
    }
}
