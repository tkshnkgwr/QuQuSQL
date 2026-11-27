#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use ququ_core::{
    ConnectCredentials, ConnectionProfile, DbType, QueryOutcome, StoragePaths,
    profile::ConnectionsFile,
};
use ququ_db::{DbCommand, DbEvent, DbSession, DbSessionFactory, DbWorker};
use ququ_db_oracle::OracleSession;
use ququ_db_sqlite::SqliteSession;
use ququ_ui::dialogs::open_exit_confirm;
use ququ_ui::state::BottomTab;
use ququ_ui::{AppAction, AppTheme, UiState, render_main_ui};
use std::fs;
use std::path::Path;
use std::sync::Arc;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

struct UnifiedDbFactory;

impl DbSessionFactory for UnifiedDbFactory {
    fn connect(
        &self,
        profile: &ConnectionProfile,
        credentials: &ConnectCredentials,
        tns_admin_override: Option<&str>,
    ) -> Result<Box<dyn DbSession>> {
        match profile.db_type {
            DbType::Sqlite => Ok(Box::new(SqliteSession::open(&profile.file_path)?)),
            DbType::Oracle => Ok(Box::new(OracleSession::connect(
                profile,
                credentials,
                tns_admin_override,
            )?)),
        }
    }
}

struct QuQuApp {
    state: UiState,
    worker: DbWorker,
    pending_profile: Option<ConnectionProfile>,
    last_dark_mode: bool,
}

impl QuQuApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_japanese_fonts(&cc.egui_ctx);

        let storage = StoragePaths::resolve();
        let config = storage
            .as_ref()
            .map(StoragePaths::load_config)
            .unwrap_or_default();
        let window_state = storage
            .as_ref()
            .map(StoragePaths::load_window_state)
            .unwrap_or_default();
        let connections = storage
            .as_ref()
            .map(StoragePaths::load_connections)
            .unwrap_or_default();

        let last_dark_mode = config.dark_mode;
        AppTheme::for_mode(last_dark_mode).apply_to_ctx(&cc.egui_ctx);

        let ctx_clone = cc.egui_ctx.clone();
        let repaint_cb: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            ctx_clone.request_repaint();
        });
        let worker = DbWorker::spawn(Arc::new(UnifiedDbFactory), repaint_cb);

        let state = UiState::new(config, window_state, connections.connections, storage);
        Self {
            state,
            worker,
            pending_profile: None,
            last_dark_mode,
        }
    }

    fn process_db_events(&mut self, ctx: &egui::Context) {
        while let Some(ev) = self.worker.try_recv() {
            self.state.busy = false;
            match ev {
                DbEvent::Connected { profile_id, db_info } => {
                    if let Some(prof) = self
                        .pending_profile
                        .take()
                        .or_else(|| self.state.profiles.iter().find(|p| p.id == profile_id).cloned())
                    {
                        self.state.read_only_session = prof.read_only;
                        self.state.active_profile = Some(prof);
                    }
                    self.state.db_info = Some(db_info.clone());
                    self.state.uncommitted_count = 0;
                    self.state.login_dialog.open = false;
                    self.state.login_dialog.error_msg = None;
                    self.state.owners.clear();
                    self.state.objects.clear();
                    self.state.columns.clear();
                    self.state.selected_object_idx = None;
                    self.state.status_notice = Some(format!("接続しました: {db_info}"));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.state.window_title()));
                    self.worker.send(DbCommand::LoadOwners);
                }
                DbEvent::Disconnected => {
                    self.state.active_profile = None;
                    self.state.db_info = None;
                    self.state.uncommitted_count = 0;
                    self.state.owners.clear();
                    self.state.objects.clear();
                    self.state.columns.clear();
                    self.state.status_notice = Some("切断しました。".to_string());
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.state.window_title()));
                }
                DbEvent::QueryFinished {
                    sql,
                    outcome,
                    has_uncommitted: _,
                    uncommitted_count,
                } => {
                    self.state.uncommitted_count = uncommitted_count;
                    match outcome {
                        QueryOutcome::Rows(rs) => {
                            let msg = format!(
                                "{} 件選択されました ({} msec.)",
                                rs.row_count, rs.elapsed_ms
                            );
                            self.state.push_log(sql, msg.clone(), false, Some(rs.elapsed_ms));
                            self.state.result = Some(rs);
                            self.state.selected_cell = None;
                            self.state.bottom_tab = BottomTab::Grid;
                            self.state.status_notice = Some(msg);
                        }
                        QueryOutcome::Affected { rows, elapsed_ms } => {
                            let msg = format!("{rows} 件処理しました ({elapsed_ms} msec.)");
                            self.state.push_log(sql, msg.clone(), false, Some(elapsed_ms));
                            self.state.bottom_tab = BottomTab::Log;
                            self.state.status_notice = Some(msg);
                        }
                        QueryOutcome::Message { text, elapsed_ms } => {
                            let msg = format!("{text} ({elapsed_ms} msec.)");
                            self.state.push_log(sql, msg.clone(), false, Some(elapsed_ms));
                            self.state.bottom_tab = BottomTab::Log;
                            self.state.status_notice = Some(msg);
                        }
                    }
                }
                DbEvent::FetchedMore { result } => {
                    let added = result.row_count;
                    if let Some(existing) = self.state.result.as_mut() {
                        existing.append(result);
                        self.state.status_notice =
                            Some(format!("追加取得しました (計 {} 件)", existing.row_count));
                    } else {
                        self.state.result = Some(result);
                        self.state.status_notice = Some(format!("{added} 件取得しました。"));
                    }
                }
                DbEvent::CommitFinished => {
                    self.state.uncommitted_count = 0;
                    self.state.push_log(
                        "COMMIT".to_string(),
                        "トランザクションをコミットしました。".to_string(),
                        false,
                        None,
                    );
                    self.state.status_notice = Some("コミット完了".to_string());
                }
                DbEvent::RollbackFinished => {
                    self.state.uncommitted_count = 0;
                    self.state.push_log(
                        "ROLLBACK".to_string(),
                        "トランザクションをロールバックしました。".to_string(),
                        false,
                        None,
                    );
                    self.state.status_notice = Some("ロールバック完了".to_string());
                }
                DbEvent::OwnersLoaded(owners) => {
                    let default_owner = self
                        .state
                        .active_profile
                        .as_ref()
                        .map(|p| p.username.trim().to_ascii_uppercase())
                        .filter(|u| owners.iter().any(|o| o.eq_ignore_ascii_case(u)))
                        .or_else(|| owners.first().cloned())
                        .unwrap_or_default();
                    self.state.owners = owners;
                    self.state.selected_owner = default_owner.clone();
                    if !default_owner.is_empty() {
                        self.worker.send(DbCommand::LoadObjects {
                            owner: default_owner,
                            object_type: self.state.selected_object_type.clone(),
                        });
                    }
                }
                DbEvent::ObjectsLoaded {
                    owner,
                    object_type,
                    objects,
                } => {
                    self.state.selected_owner = owner;
                    self.state.selected_object_type = object_type;
                    self.state.objects = objects;
                    self.state.selected_object_idx = None;
                    self.state.columns.clear();
                }
                DbEvent::ColumnsLoaded {
                    owner: _,
                    table_name: _,
                    columns,
                } => {
                    self.state.columns = columns;
                }
                DbEvent::DdlLoaded { .. } => {}
                DbEvent::Error { context, message } => {
                    if context == "接続エラー" {
                        self.pending_profile = None;
                        self.state.login_dialog.error_msg = Some(message.clone());
                    }
                    self.state.push_log(context, format!("Error: {message}"), true, None);
                    self.state.bottom_tab = BottomTab::Log;
                    self.state.status_notice = Some("エラーが発生しました（ログ確認）".to_string());
                }
            }
        }
    }

    fn dispatch_actions(&mut self, ctx: &egui::Context, actions: Vec<AppAction>) {
        for action in actions {
            match action {
                AppAction::Connect {
                    profile,
                    credentials,
                    tns_admin_override,
                } => {
                    self.pending_profile = Some(profile.clone());
                    self.state.busy = true;
                    self.worker.send(DbCommand::Connect {
                        profile,
                        credentials,
                        tns_admin_override,
                    });
                }
                AppAction::Disconnect => {
                    self.worker.send(DbCommand::Disconnect);
                }
                AppAction::ExecuteSql { sql, allow_write } => {
                    self.state.busy = true;
                    self.worker.send(DbCommand::ExecuteSql {
                        sql,
                        fetch_limit: self.state.config.default_fetch_limit,
                        allow_write,
                    });
                }
                AppAction::FetchMore { sql, offset, limit } => {
                    self.state.busy = true;
                    self.worker.send(DbCommand::FetchMore { sql, offset, limit });
                }
                AppAction::Commit => {
                    self.state.busy = true;
                    self.worker.send(DbCommand::Commit);
                }
                AppAction::Rollback => {
                    self.state.busy = true;
                    self.worker.send(DbCommand::Rollback);
                }
                AppAction::LoadOwners => {
                    self.worker.send(DbCommand::LoadOwners);
                }
                AppAction::LoadObjects { owner, object_type } => {
                    self.worker.send(DbCommand::LoadObjects { owner, object_type });
                }
                AppAction::LoadColumns { owner, table_name } => {
                    self.worker.send(DbCommand::LoadColumns { owner, table_name });
                }
                AppAction::SaveProfiles => {
                    if let Some(storage) = &self.state.storage {
                        let file = ConnectionsFile {
                            connections: self.state.profiles.clone(),
                        };
                        let _ = storage.save_connections(&file);
                    }
                }
                AppAction::SaveConfig => {
                    if let Some(storage) = &self.state.storage {
                        let _ = storage.save_config(&self.state.config);
                    }
                }
                AppAction::ConfirmExitClose => {
                    self.state.allow_force_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }
}

impl eframe::App for QuQuApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_db_events(ctx);

        if self.last_dark_mode != self.state.config.dark_mode {
            self.last_dark_mode = self.state.config.dark_mode;
            AppTheme::for_mode(self.last_dark_mode).apply_to_ctx(ctx);
        }

        // 未コミット変更がある状態でのウィンドウ終了をガード
        if ctx.input(|i| i.viewport().close_requested())
            && !self.state.allow_force_exit
            && self.state.uncommitted_count > 0
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            open_exit_confirm(&mut self.state);
        }

        let theme = AppTheme::for_mode(self.state.config.dark_mode);
        let actions = render_main_ui(ctx, &mut self.state, &theme);
        if !actions.is_empty() {
            self.dispatch_actions(ctx, actions);
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.state.window_title()));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(storage) = &self.state.storage {
            let _ = storage.save_window_state(&self.state.window_state);
            let _ = storage.save_config(&self.state.config);
        }
    }
}

fn setup_japanese_fonts(ctx: &egui::Context) {
    let candidates = [
        r"C:\Windows\Fonts\BIZ-UDGothicR.ttc",
        r"C:\Windows\Fonts\meiryo.ttc",
        r"C:\Windows\Fonts\YuGothR.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
    ];

    for path in candidates {
        if Path::new(path).is_file() {
            if let Ok(bytes) = fs::read(path) {
                let mut fonts = FontDefinitions::default();
                fonts
                    .font_data
                    .insert("jp_system".to_owned(), Arc::new(FontData::from_owned(bytes)));

                if let Some(fam) = fonts.families.get_mut(&FontFamily::Proportional) {
                    fam.insert(0, "jp_system".to_owned());
                }
                if let Some(fam) = fonts.families.get_mut(&FontFamily::Monospace) {
                    fam.push("jp_system".to_owned());
                }
                ctx.set_fonts(fonts);
                break;
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    let initial_ws = StoragePaths::resolve()
        .map(|s| s.load_window_state())
        .unwrap_or_default();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("QuQuSQL")
            .with_inner_size([initial_ws.width, initial_ws.height])
            .with_min_inner_size([860.0, 560.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "QuQuSQL",
        native_options,
        Box::new(|cc| Ok(Box::new(QuQuApp::new(cc)))),
    )
}
