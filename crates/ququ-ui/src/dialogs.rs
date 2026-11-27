//! 接続・ログインダイアログ、安全確認モーダル、およびバージョン情報ダイアログの描画モジュール。

use crate::state::{AppAction, ConfirmAction, ConfirmModal, UiState};
use crate::theme::{AppTheme, env_color};
use egui::{Align, Color32, RichText, Stroke, Ui};
use ququ_core::{ConnectCredentials, ConnectionProfile, DbType, EnvironmentTier, OracleConnectMode};

/// 接続・ログインおよび接続プロファイル管理ダイアログを描画する。
pub fn render_login_dialog(
    ctx: &egui::Context,
    state: &mut UiState,
    _theme: &AppTheme,
    actions: &mut Vec<AppAction>,
) {
    if !state.login_dialog.open {
        return;
    }

    let mut open = state.login_dialog.open;
    egui::Window::new("データベース接続・ログイン")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([720.0, 480.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.columns(2, |cols| {
                // 左列：環境区分ごとの接続一覧（誤接続防止のため本番環境は赤枠分離）
                render_profile_list_column(&mut cols[0], state);

                // 右列：ログイン／プロファイル編集フォーム
                render_profile_detail_column(&mut cols[1], state, actions);
            });
        });

    if !open {
        state.login_dialog.open = false;
        state.login_dialog.password_input.clear();
    }
}

/// 左列：環境区分ごとに整理された接続プロファイル一覧を描画する。
fn render_profile_list_column(ui: &mut Ui, state: &mut UiState) {
    ui.horizontal(|ui| {
        ui.heading("接続先一覧");
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            if ui.button("+ 新規作成").clicked() {
                let tmpl =
                    ConnectionProfile::new_template(EnvironmentTier::Development, DbType::Oracle);
                state.login_dialog.editing_profile = Some(tmpl);
                state.login_dialog.is_creating_new = true;
                state.login_dialog.error_msg = None;
            }
        });
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt("login_profile_list")
        .max_height(390.0)
        .show(ui, |ui| {
            for tier in [
                EnvironmentTier::Local,
                EnvironmentTier::Development,
                EnvironmentTier::Staging,
            ] {
                render_tier_group(ui, state, tier);
            }

            // 本番環境は赤枠で物理的に分離し、不用意なクリックを防止する
            ui.add_space(6.0);
            let prod_color = env_color(EnvironmentTier::Production);
            egui::Frame::NONE
                .stroke(Stroke::new(1.5_f32, prod_color))
                .corner_radius(4.0)
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| {
                    let prod_profiles: Vec<ConnectionProfile> = state
                        .profiles
                        .iter()
                        .filter(|p| p.env == EnvironmentTier::Production)
                        .cloned()
                        .collect();

                    let header_text = RichText::new(format!(
                        "⚠ 本番環境 ({}件) - 接続注意",
                        prod_profiles.len()
                    ))
                    .color(prod_color)
                    .strong();

                    let collapsing = egui::CollapsingHeader::new(header_text)
                        .id_salt("prod_section_header")
                        .default_open(state.login_dialog.production_expanded)
                        .show(ui, |ui| {
                            if prod_profiles.is_empty() {
                                ui.label(RichText::new("登録なし").small().color(Color32::GRAY));
                            } else {
                                for prof in &prod_profiles {
                                    render_profile_row(ui, state, prof);
                                }
                            }
                        });
                    state.login_dialog.production_expanded = collapsing.fully_open();
                });
        });
}

/// 指定された環境区分に属する接続プロファイル一覧を描画する。
fn render_tier_group(ui: &mut Ui, state: &mut UiState, tier: EnvironmentTier) {
    let items: Vec<ConnectionProfile> = state
        .profiles
        .iter()
        .filter(|p| p.env == tier)
        .cloned()
        .collect();
    if items.is_empty() {
        return;
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let color = env_color(tier);
        egui::Frame::NONE
            .fill(color)
            .corner_radius(3.0)
            .inner_margin(egui::Margin::symmetric(5, 1))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(tier.label())
                        .small()
                        .strong()
                        .color(Color32::WHITE),
                );
            });
        ui.label(RichText::new(format!("({}件)", items.len())).small().color(Color32::GRAY));
    });

    for prof in &items {
        render_profile_row(ui, state, prof);
    }
}

/// 接続プロファイル一覧の 1 行を描画する。
fn render_profile_row(ui: &mut Ui, state: &mut UiState, prof: &ConnectionProfile) {
    let selected = state.login_dialog.selected_profile_id.as_deref() == Some(&prof.id);
    let color = env_color(prof.env);
    let label = format!("{}  [{}]", prof.name, prof.db_type.label());

    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(4.0, 20.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 1.0, color);

        if ui.selectable_label(selected, label).clicked() {
            if !selected {
                state.login_dialog.password_input.clear();
            }
            state.login_dialog.selected_profile_id = Some(prof.id.clone());
            state.login_dialog.editing_profile = None;
            state.login_dialog.is_creating_new = false;
            state.login_dialog.error_msg = None;
        }
    });
}

/// 右列：選択中プロファイルの接続実行画面（または編集フォーム）を描画する。
fn render_profile_detail_column(
    ui: &mut Ui,
    state: &mut UiState,
    actions: &mut Vec<AppAction>,
) {
    if state.login_dialog.editing_profile.is_some() {
        render_profile_editor(ui, state, actions);
        return;
    }

    let selected = state
        .login_dialog
        .selected_profile_id
        .as_ref()
        .and_then(|id| state.profiles.iter().find(|p| &p.id == id))
        .cloned();

    let Some(prof) = selected else {
        ui.centered_and_justified(|ui| {
            ui.label("左の一覧から接続先を選択するか、「+ 新規作成」を押してください。");
        });
        return;
    };

    let color = env_color(prof.env);
    ui.horizontal(|ui| {
        egui::Frame::NONE
            .fill(color)
            .corner_radius(3.0)
            .inner_margin(egui::Margin::symmetric(6, 2))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(prof.env.badge_text())
                        .strong()
                        .color(Color32::WHITE),
                );
            });
        ui.heading(&prof.name);
    });
    ui.separator();

    if prof.env.is_production() {
        egui::Frame::NONE
            .fill(Color32::from_rgb(255, 235, 233))
            .stroke(Stroke::new(1.0_f32, color))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("⚠ これは本番環境のプロファイルです。\n接続時は読み取り専用モードで開始され、コミットや更新時に必ず確認ダイアログが表示されます。")
                        .small()
                        .color(Color32::from_rgb(160, 20, 30)),
                );
            });
        ui.add_space(6.0);
    }

    egui::Grid::new("login_summary_grid")
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            ui.label("DB種別:");
            ui.label(prof.db_type.label());
            ui.end_row();

            ui.label("接続先:");
            ui.label(prof.display_target());
            ui.end_row();

            ui.label("初期モード:");
            ui.label(if prof.read_only {
                "読み取り専用 (Read-Only)"
            } else {
                "通常 (読み書き可)"
            });
            ui.end_row();
        });

    ui.add_space(8.0);

    if prof.db_type == DbType::Oracle {
        ui.label("パスワード（接続時都度入力・ディスク保存なし）:");
        let pw_resp = ui.add(
            egui::TextEdit::singleline(&mut state.login_dialog.password_input)
                .password(true)
                .desired_width(f32::INFINITY)
                .hint_text("パスワードを入力..."),
        );
        if pw_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            trigger_connect(state, &prof, actions);
        }
        ui.add_space(8.0);
    }

    if let Some(err) = &state.login_dialog.error_msg {
        ui.label(RichText::new(err).color(Color32::from_rgb(209, 36, 47)));
        ui.add_space(4.0);
    }

    ui.horizontal(|ui| {
        let btn_text = if prof.env.is_production() {
            RichText::new("⚠ 本番環境へ接続")
                .color(Color32::WHITE)
                .strong()
        } else {
            RichText::new("接続").color(Color32::WHITE).strong()
        };
        let connect_btn = egui::Button::new(btn_text).fill(color);
        if ui.add(connect_btn).clicked() {
            trigger_connect(state, &prof, actions);
        }

        if ui.button("編集").clicked() {
            state.login_dialog.editing_profile = Some(prof.clone());
            state.login_dialog.is_creating_new = false;
            state.login_dialog.error_msg = None;
        }

        if state.profiles.len() > 1 && ui.button("削除").clicked() {
            state.profiles.retain(|p| p.id != prof.id);
            state.login_dialog.selected_profile_id = state.profiles.first().map(|p| p.id.clone());
            actions.push(AppAction::SaveProfiles);
        }
    });
}

/// 接続要求アクションを発行し、入力中パスワードをムーブ後に消去する。
fn trigger_connect(state: &mut UiState, prof: &ConnectionProfile, actions: &mut Vec<AppAction>) {
    let pw = if prof.db_type == DbType::Oracle {
        Some(std::mem::take(&mut state.login_dialog.password_input))
    } else {
        None
    };
    let tns_override = if state.config.tns_admin_path.trim().is_empty() {
        None
    } else {
        Some(state.config.tns_admin_path.clone())
    };
    actions.push(AppAction::Connect {
        profile: prof.clone(),
        credentials: ConnectCredentials { password: pw },
        tns_admin_override: tns_override,
    });
    state.login_dialog.error_msg = None;
}

/// 接続プロファイルの新規作成・編集フォームを描画する。
fn render_profile_editor(ui: &mut Ui, state: &mut UiState, actions: &mut Vec<AppAction>) {
    let Some(mut edit) = state.login_dialog.editing_profile.take() else {
        return;
    };

    ui.heading(if state.login_dialog.is_creating_new {
        "新規接続プロファイル作成"
    } else {
        "接続プロファイル編集"
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt("profile_editor_scroll")
        .max_height(340.0)
        .show(ui, |ui| {
            egui::Grid::new("profile_edit_grid")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label("接続表示名:");
                    ui.text_edit_singleline(&mut edit.name);
                    ui.end_row();

                    ui.label("環境区分:");
                    let prev_env = edit.env;
                    egui::ComboBox::from_id_salt("edit_env_combo")
                        .selected_text(edit.env.badge_text())
                        .show_ui(ui, |ui| {
                            for &env in EnvironmentTier::all() {
                                ui.selectable_value(&mut edit.env, env, env.badge_text());
                            }
                        });
                    // 環境区分を切り替えた場合は安全プリセットを自動反映する
                    if edit.env != prev_env {
                        edit.apply_env_presets(edit.env);
                    }
                    ui.end_row();

                    ui.label("DB種別:");
                    egui::ComboBox::from_id_salt("edit_dbtype_combo")
                        .selected_text(edit.db_type.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut edit.db_type, DbType::Oracle, "Oracle");
                            ui.selectable_value(&mut edit.db_type, DbType::Sqlite, "SQLite");
                        });
                    ui.end_row();

                    match edit.db_type {
                        DbType::Sqlite => {
                            ui.label("DBファイル:");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut edit.file_path)
                                        .desired_width(170.0)
                                        .hint_text(":memory: または .db パス"),
                                );
                                if ui.button("参照...").clicked()
                                    && let Some(path) = rfd::FileDialog::new()
                                        .add_filter("SQLite DB", &["db", "sqlite", "sqlite3"])
                                        .pick_file()
                                {
                                    edit.file_path = path.display().to_string();
                                }
                            });
                            ui.end_row();
                        }
                        DbType::Oracle => {
                            ui.label("接続方式:");
                            ui.horizontal(|ui| {
                                ui.radio_value(
                                    &mut edit.connect_mode,
                                    OracleConnectMode::Direct,
                                    "直接指定",
                                );
                                ui.radio_value(
                                    &mut edit.connect_mode,
                                    OracleConnectMode::Tns,
                                    "tnsnames.ora",
                                );
                            });
                            ui.end_row();

                            match edit.connect_mode {
                                OracleConnectMode::Direct => {
                                    ui.label("ホスト:");
                                    ui.text_edit_singleline(&mut edit.host);
                                    ui.end_row();

                                    ui.label("ポート:");
                                    let mut port_str = edit.port.to_string();
                                    if ui.text_edit_singleline(&mut port_str).changed()
                                        && let Ok(p) = port_str.trim().parse::<u16>()
                                    {
                                        edit.port = p;
                                    }
                                    ui.end_row();

                                    ui.label("サービス名:");
                                    ui.text_edit_singleline(&mut edit.service_name);
                                    ui.end_row();
                                }
                                OracleConnectMode::Tns => {
                                    ui.label("TNS 名:");
                                    ui.text_edit_singleline(&mut edit.tns_name);
                                    ui.end_row();
                                }
                            }

                            ui.label("ユーザー名:");
                            ui.text_edit_singleline(&mut edit.username);
                            ui.end_row();
                        }
                    }

                    ui.label("安全オプション:");
                    ui.vertical(|ui| {
                        ui.checkbox(&mut edit.read_only, "読み取り専用 (Read-Only) で接続");
                        ui.checkbox(&mut edit.confirm_on_commit, "コミット時に確認ダイアログ");
                        ui.checkbox(&mut edit.confirm_on_dml_ddl, "DML / DDL 実行時に確認");
                    });
                    ui.end_row();
                });
        });

    ui.add_space(8.0);
    let mut keep_editing = true;
    ui.horizontal(|ui| {
        if ui.button("保存").clicked() {
            if edit.name.trim().is_empty() {
                edit.name = "無題の接続".to_string();
            }
            if let Some(existing) = state.profiles.iter_mut().find(|p| p.id == edit.id) {
                *existing = edit.clone();
            } else {
                state.profiles.push(edit.clone());
            }
            state.login_dialog.selected_profile_id = Some(edit.id.clone());
            if edit.env == EnvironmentTier::Production {
                state.login_dialog.production_expanded = true;
            }
            actions.push(AppAction::SaveProfiles);
            keep_editing = false;
        }
        if ui.button("キャンセル").clicked() {
            keep_editing = false;
        }
    });

    if keep_editing {
        state.login_dialog.editing_profile = Some(edit);
    }
}

/// 安全確認モーダルを描画する。
///
/// 誤って Enter キーを連打しても確定されないよう、
/// キャンセルボタンをデフォルトフォーカス（かつ Esc で閉じる）に固定する。
pub fn render_confirm_modal(
    ctx: &egui::Context,
    state: &mut UiState,
    actions: &mut Vec<AppAction>,
) {
    let Some(modal) = state.confirm_modal.clone() else {
        return;
    };

    let mut close_modal = false;
    let mut confirmed = false;

    let accent = if modal.is_danger {
        Color32::from_rgb(209, 36, 47)
    } else {
        Color32::from_rgb(191, 135, 0)
    };

    egui::Window::new(format!("⚠ {}", modal.title))
        .collapsible(false)
        .resizable(false)
        .default_width(460.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(egui::Frame::window(&ctx.style()).stroke(Stroke::new(2.0_f32, accent)))
        .show(ctx, |ui| {
            ui.label(RichText::new(&modal.message).strong());

            if let Some(sql) = &modal.sql_preview {
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .id_salt("confirm_sql_preview")
                    .max_height(130.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut sql.clone())
                                .font(egui::FontId::monospace(12.0))
                                .desired_width(f32::INFINITY)
                                .interactive(false),
                        );
                    });
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                // キャンセルボタンを先頭かつデフォルトフォーカスに固定して誤爆を防止する
                let cancel_btn = ui.button("キャンセル (安全)");
                if cancel_btn.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close_modal = true;
                }
                if !cancel_btn.has_focus() && ui.memory(|m| m.focused().is_none()) {
                    cancel_btn.request_focus();
                }

                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let ok_btn = egui::Button::new(
                        RichText::new(&modal.confirm_label)
                            .color(Color32::WHITE)
                            .strong(),
                    )
                    .fill(accent);
                    if ui.add(ok_btn).clicked() {
                        confirmed = true;
                        close_modal = true;
                    }
                });
            });
        });

    if confirmed {
        execute_confirmed_action(state, modal.action, actions);
    }
    if close_modal {
        state.confirm_modal = None;
    }
}

/// 確認モーダル承認後のアクションを `AppAction` へ変換して発行する。
fn execute_confirmed_action(
    state: &mut UiState,
    action: ConfirmAction,
    actions: &mut Vec<AppAction>,
) {
    match action {
        ConfirmAction::ProductionCommit { .. } => {
            actions.push(AppAction::Commit);
        }
        ConfirmAction::ProductionSql { sql, .. }
        | ConfirmAction::DmlWithoutWhere { sql }
        | ConfirmAction::ImplicitCommitDdl { sql, .. } => {
            actions.push(AppAction::ExecuteSql {
                sql,
                allow_write: !state.read_only_session,
            });
        }
        ConfirmAction::ExitWithUncommitted { .. } => {
            actions.push(AppAction::ConfirmExitClose);
        }
        ConfirmAction::ApplyGridDml { dmls } => {
            actions.push(AppAction::ApplyGridDml {
                dmls,
                allow_write: !state.read_only_session,
            });
        }
    }
}

/// 未コミット変更または未保存 SQL タブがある状態でのアプリ終了確認モーダルを開く。
pub fn open_exit_confirm(state: &mut UiState) {
    let dirty_tabs = state.dirty_tab_titles();
    let has_uncommitted = state.uncommitted_count > 0;
    let has_dirty_tabs = !dirty_tabs.is_empty();

    let title = match (has_uncommitted, has_dirty_tabs) {
        (true, true) => "未コミット変更と未保存の SQL タブがあります",
        (true, false) => "未コミットの変更があります",
        (false, true) => "未保存の SQL タブがあります",
        (false, false) => "終了確認",
    }
    .to_string();

    let mut lines = Vec::new();
    if has_uncommitted {
        lines.push(format!(
            "・未コミットの DB 変更が {} 件あります（終了するとロールバックされます）。",
            state.uncommitted_count
        ));
    }
    if has_dirty_tabs {
        lines.push(format!(
            "・未保存の SQL タブが {} 件あります: {}",
            dirty_tabs.len(),
            dirty_tabs.join(", ")
        ));
    }
    lines.push("このまま保存・コミットせずに終了してもよろしいですか？".to_string());

    state.confirm_modal = Some(ConfirmModal {
        title,
        message: lines.join("\n"),
        sql_preview: None,
        is_danger: state.is_production() || has_uncommitted,
        confirm_label: "破棄して終了".to_string(),
        action: ConfirmAction::ExitWithUncommitted {
            uncommitted_count: state.uncommitted_count,
        },
    });
}

/// ヘルプメニューから開くバージョン情報ダイアログを描画する。
///
/// バージョン番号はソースにハードコーディングせず、`Cargo.toml` 由来の `state.app_version` を表示する。
pub fn render_version_dialog(ctx: &egui::Context, state: &mut UiState) {
    if !state.show_version_dialog {
        return;
    }

    let mut open = state.show_version_dialog;
    egui::Window::new("バージョン情報")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(380.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.heading("QuQuSQL");
            ui.label(RichText::new("Quick Query SQL Tools").small().color(Color32::GRAY));
            ui.separator();

            egui::Grid::new("version_info_grid")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label("バージョン:");
                    ui.label(RichText::new(format!("v{}", state.app_version)).strong());
                    ui.end_row();

                    ui.label("対応 RDBMS:");
                    ui.label("Oracle Database (Pure Rust) / SQLite");
                    ui.end_row();

                    ui.label("UI エンジン:");
                    ui.label("Rust + egui / eframe (glow)");
                    ui.end_row();
                });

            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.button("閉じる").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    state.show_version_dialog = false;
                }
            });
        });

    if !open {
        state.show_version_dialog = false;
    }
}
