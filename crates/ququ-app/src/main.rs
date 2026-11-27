//! QuQuSQL デスクトップアプリケーションのエントリポイント。
//!
//! `mimalloc` アロケータ、Windows 日本語フォント読込、`eframe` (`glow`) 起動、
//! および UI 層（`ququ-ui`）と非同期 DB ワーカー（`ququ-db`）間のイベント連携を担当します。

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
use ququ_ui::browser::format_full_select_sql;
use ququ_ui::dialogs::open_exit_confirm;
use ququ_ui::state::BottomTab;
use ququ_ui::{AppAction, AppTheme, UiState, render_main_ui};
use std::fs;
use std::path::Path;
use std::sync::Arc;

/// 高速描画・大量データ処理時のアロケーション遅延を抑えるグローバルアロケータ。
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// SQLite / Oracle 両対応の統一セッションファクトリ。
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

/// アプリケーション全体の実行時コンテキスト。
struct QuQuApp {
    state: UiState,
    worker: DbWorker,
    pending_profile: Option<ConnectionProfile>,
    last_dark_mode: bool,
}

impl QuQuApp {
    /// 設定・接続プロファイルを読み込み、DB ワーカーを起動して初期化する。
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

    /// バックグラウンド DB ワーカーから届いたイベントを処理し UI 状態へ反映する。
    fn process_db_events(&mut self, ctx: &egui::Context) {
        while let Some(ev) = self.worker.try_recv() {
            self.state.busy = false;
            match ev {
                DbEvent::Connected {
                    profile_id,
                    db_info,
                } => {
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
                    self.state.pending_select_insert_table = None;
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
                    self.state.pending_select_insert_table = None;
                    self.state.status_notice = Some("切断しました。".to_string());
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.state.window_title()));
                }
                DbEvent::QueryFinished {
                    sql,
                    outcome,
                    is_ddl,
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

                    // DDL 実行時にサイドバーのオブジェクト一覧が表示中であれば自動で再読込する
                    if is_ddl
                        && self.state.window_state.show_browser
                        && !self.state.selected_owner.is_empty()
                    {
                        self.worker.send(DbCommand::LoadObjects {
                            owner: self.state.selected_owner.clone(),
                            object_type: self.state.selected_object_type.clone(),
                        });
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
                    table_name,
                    columns,
                } => {
                    self.state.columns = columns;
                    // 右クリック「SELECT 文を挿入 (全項目)」の待機中テーブルであればここでエディタへ展開する
                    if let Some(pending_table) = self.state.pending_select_insert_table.take()
                        && pending_table.eq_ignore_ascii_case(&table_name)
                    {
                        let sql = format_full_select_sql(&table_name, &self.state.columns);
                        self.state.insert_into_active_editor(&sql);
                        self.state.status_notice =
                            Some(format!("{table_name} の全項目 SELECT 文を挿入しました。"));
                    }
                }
                DbEvent::DdlLoaded { name, ddl } => {
                    // 取得した DDL を整形してアクティブな SQL エディタへ貼り付ける
                    let trimmed = ddl.trim();
                    let snippet = if trimmed.is_empty() {
                        format!("-- DDL が見つかりません: {name}\n")
                    } else if trimmed.ends_with(';') {
                        format!("{trimmed}\n")
                    } else {
                        format!("{trimmed};\n")
                    };
                    self.state.insert_into_active_editor(&snippet);
                    self.state.status_notice =
                        Some(format!("{name} の DDL をエディタへ貼り付けました。"));
                }
                DbEvent::Error { context, message } => {
                    if context == "接続エラー" {
                        self.pending_profile = None;
                        self.state.login_dialog.error_msg = Some(message.clone());
                    }
                    self.state.pending_select_insert_table = None;
                    self.state
                        .push_log(context, format!("Error: {message}"), true, None);
                    self.state.bottom_tab = BottomTab::Log;
                    self.state.status_notice = Some("エラーが発生しました（ログ確認）".to_string());
                }
            }
        }
    }

    /// UI 層から受け取ったアクションを DB ワーカーコマンドや永続化処理へ振り分ける。
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
                AppAction::LoadDdl {
                    owner,
                    object_type,
                    name,
                } => {
                    self.state.busy = true;
                    self.worker.send(DbCommand::LoadDdl {
                        owner,
                        object_type,
                        name,
                    });
                }
                AppAction::InsertFullSelect { owner, table_name } => {
                    self.state.pending_select_insert_table = Some(table_name.clone());
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
    /// 毎フレーム（イベント駆動）で DB イベント処理・終了ガード・UI 描画を実行する。
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_db_events(ctx);

        if self.last_dark_mode != self.state.config.dark_mode {
            self.last_dark_mode = self.state.config.dark_mode;
            AppTheme::for_mode(self.last_dark_mode).apply_to_ctx(ctx);
        }

        // 未コミット変更がある状態でのウィンドウ終了をガードする
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

    /// アプリ終了時にウィンドウサイズ・ペイン分割比・設定を保存する。
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(storage) = &self.state.storage {
            let _ = storage.save_window_state(&self.state.window_state);
            let _ = storage.save_config(&self.state.config);
        }
    }
}

/// Windows システム標準の日本語フォントを優先順で探索し `egui` へ登録する。
fn setup_japanese_fonts(ctx: &egui::Context) {
    let candidates = [
        r"C:\Windows\Fonts\BIZ-UDGothicR.ttc",
        r"C:\Windows\Fonts\meiryo.ttc",
        r"C:\Windows\Fonts\YuGothR.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
    ];

    for path in candidates {
        if Path::new(path).is_file()
            && let Ok(bytes) = fs::read(path)
        {
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

/// アプリケーション起動エントリポイント（描画バックエンド: `glow`）。
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
