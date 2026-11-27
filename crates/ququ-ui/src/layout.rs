//! メインウィンドウ全体の 2×2 スプリットペイン・メニュー・ツールバー・本番警告バナーを描画するモジュール。

use crate::browser::{render_object_detail_pane, render_object_list_pane};
use crate::dialogs::{render_confirm_modal, render_login_dialog, render_version_dialog};
use crate::editor::render_editor_pane;
use crate::grid::render_result_grid;
use crate::state::{AppAction, BottomTab, ConfirmAction, ConfirmModal, SqlTab, UiState};
use crate::theme::{AppTheme, env_color, profile_accent_color};
use egui::{Align, Color32, Key, Modifiers, RichText, Sense, Ui};
use ququ_sql::{StatementKind, analyze_statement, is_dml_without_where, statement_at_offset};
use std::fs;

/// メイン UI 全体を描画し、発生したユーザーアクション一覧を返す。
pub fn render_main_ui(ctx: &egui::Context, state: &mut UiState, theme: &AppTheme) -> Vec<AppAction> {
    let mut actions = Vec::new();

    handle_shortcuts(ctx, state, &mut actions);

    // 1. 最上部の環境識別アクセントライン + メニューバー
    egui::TopBottomPanel::top("menu_bar_panel").show(ctx, |ui| {
        if let Some(prof) = &state.active_profile {
            let accent = profile_accent_color(prof);
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 3.0), Sense::hover());
            ui.painter().rect_filled(rect, 0.0, accent);
        }

        egui::menu::bar(ui, |ui| {
            ui.menu_button("ファイル(F)", |ui| {
                if ui.button("新規 SQL タブ (Ctrl+N)").clicked() {
                    state.add_new_tab();
                    ui.close_menu();
                }
                if ui.button("SQL ファイルを開く... (Ctrl+O)").clicked() {
                    open_sql_file(state);
                    ui.close_menu();
                }
                if ui.button("保存 (Ctrl+S)").clicked() {
                    save_active_tab(state, false);
                    ui.close_menu();
                }
                if ui.button("名前を付けて保存...").clicked() {
                    save_active_tab(state, true);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("終了").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    ui.close_menu();
                }
            });

            ui.menu_button("編集(E)", |ui| {
                if ui.button("検索... (Ctrl+F)").clicked() {
                    state.find_replace.open = true;
                    state.find_replace.show_replace = false;
                    ui.close_menu();
                }
                if ui.button("置換... (Ctrl+H)").clicked() {
                    state.find_replace.open = true;
                    state.find_replace.show_replace = true;
                    ui.close_menu();
                }
            });

            ui.menu_button("SQL(Q)", |ui| {
                let connected = state.active_profile.is_some();
                if ui
                    .add_enabled(
                        connected && !state.busy,
                        egui::Button::new("▶ 実行 (F5 / Ctrl+Enter)"),
                    )
                    .clicked()
                {
                    request_execute_current_sql(state, &mut actions);
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        connected && !state.busy && !state.read_only_session,
                        egui::Button::new("Commit"),
                    )
                    .clicked()
                {
                    request_commit(state, &mut actions);
                    ui.close_menu();
                }
                if ui
                    .add_enabled(connected && !state.busy, egui::Button::new("Rollback"))
                    .clicked()
                {
                    actions.push(AppAction::Rollback);
                    ui.close_menu();
                }
            });

            ui.menu_button("表示(V)", |ui| {
                if ui
                    .checkbox(&mut state.window_state.show_browser, "オブジェクトブラウザ (F8)")
                    .clicked()
                {
                    ui.close_menu();
                }
                if ui
                    .checkbox(&mut state.config.dark_mode, "ダークテーマ")
                    .clicked()
                {
                    actions.push(AppAction::SaveConfig);
                    ui.close_menu();
                }
            });

            ui.menu_button("接続(C)", |ui| {
                if ui.button("接続 / 切替...").clicked() {
                    state.login_dialog.open = true;
                    ui.close_menu();
                }
                if ui
                    .add_enabled(state.active_profile.is_some(), egui::Button::new("切断"))
                    .clicked()
                {
                    actions.push(AppAction::Disconnect);
                    ui.close_menu();
                }
            });

            ui.menu_button("ヘルプ(H)", |ui| {
                if ui.button("バージョン情報...").clicked() {
                    state.show_version_dialog = true;
                    ui.close_menu();
                }
            });
        });
    });

    // 2. 本番環境接続時の赤系常時警告バナー（誤爆防止の要）
    if let Some(prof) = state.active_profile.clone()
        && prof.env.is_production()
    {
        egui::TopBottomPanel::top("prod_warning_banner")
            .frame(
                egui::Frame::NONE
                    .fill(Color32::from_rgb(195, 25, 35))
                    .inner_margin(egui::Margin::symmetric(10, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "⚠ 本番環境に接続中: {} ({})   [状態: {}]",
                            prof.name,
                            prof.display_target(),
                            if state.read_only_session {
                                "読み取り専用"
                            } else {
                                "更新許可 (注意!)"
                            }
                        ))
                        .strong()
                        .color(Color32::WHITE),
                    );
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let toggle_text = if state.read_only_session {
                            "更新ロックを解除"
                        } else {
                            "読み取り専用に戻す"
                        };
                        if ui.button(RichText::new(toggle_text).small().strong()).clicked() {
                            state.read_only_session = !state.read_only_session;
                        }
                    });
                });
            });
    }

    // 3. ツールバー
    egui::TopBottomPanel::top("toolbar_panel").show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui.button("新規").clicked() {
                state.add_new_tab();
            }
            if ui.button("開く").clicked() {
                open_sql_file(state);
            }
            if ui.button("保存").clicked() {
                save_active_tab(state, false);
            }
            ui.separator();

            let connected = state.active_profile.is_some();
            if ui
                .add_enabled(connected && !state.busy, egui::Button::new("▶ 実行 (F5)"))
                .clicked()
            {
                request_execute_current_sql(state, &mut actions);
            }
            if ui
                .add_enabled(
                    connected && !state.busy && !state.read_only_session,
                    egui::Button::new("Commit"),
                )
                .clicked()
            {
                request_commit(state, &mut actions);
            }
            if ui
                .add_enabled(connected && !state.busy, egui::Button::new("Rollback"))
                .clicked()
            {
                actions.push(AppAction::Rollback);
            }
            ui.separator();

            if ui.button("検索/置換").clicked() {
                state.find_replace.open = !state.find_replace.open;
            }
            if ui.button("ブラウザ(F8)").clicked() {
                state.window_state.show_browser = !state.window_state.show_browser;
            }

            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.button("接続設定...").clicked() {
                    state.login_dialog.open = true;
                }
                if let Some(prof) = &state.active_profile {
                    let badge_bg = profile_accent_color(prof);
                    egui::Frame::NONE
                        .fill(badge_bg)
                        .corner_radius(3.0)
                        .inner_margin(egui::Margin::symmetric(8, 2))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("{} {}", prof.env.badge_text(), prof.name))
                                    .small()
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                        });
                }
            });
        });
    });

    // 4. 最下部ステータスバー
    egui::TopBottomPanel::bottom("status_bar_panel").show(ctx, |ui| {
        ui.horizontal(|ui| {
            if let Some(tab) = state.active_tab_ref() {
                ui.label(RichText::new(&tab.encoding_label).small());
                ui.separator();
            }

            match &state.active_profile {
                Some(prof) => {
                    let label = format!("● {}", prof.display_target());
                    let color = if prof.env.is_production() {
                        Color32::from_rgb(209, 36, 47)
                    } else {
                        env_color(prof.env)
                    };
                    ui.label(RichText::new(label).small().strong().color(color));
                }
                None => {
                    ui.label(RichText::new("○ 未接続").small().color(Color32::GRAY));
                }
            }
            ui.separator();

            if state.uncommitted_count > 0 {
                ui.label(
                    RichText::new(format!("Tx: 未コミット ({}件)", state.uncommitted_count))
                        .small()
                        .strong()
                        .color(Color32::from_rgb(191, 100, 0)),
                );
            } else {
                ui.label(RichText::new("Tx: Clean").small().color(Color32::GRAY));
            }
            ui.separator();

            if state.busy {
                ui.spinner();
                ui.label(RichText::new("実行中...").small());
            } else if let Some(notice) = &state.status_notice {
                ui.label(RichText::new(notice).small());
            }

            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if let Some(tab) = state.active_tab_ref() {
                    ui.label(
                        RichText::new(format!("{} 行, {} 列", tab.cursor_line, tab.cursor_col))
                            .small(),
                    );
                }
            });
        });
    });

    // 5. メイン 2×2 スプリットペイン
    egui::CentralPanel::default().show(ctx, |ui| {
        let total_rect = ui.available_rect_before_wrap();
        let splitter_thickness = 5.0;

        if state.window_state.show_browser {
            let left_w = (total_rect.width() * state.window_state.left_panel_ratio)
                .clamp(280.0, (total_rect.width() - 220.0).max(280.0));
            let split_x = total_rect.left() + left_w;
            let right_w = (total_rect.width() - left_w - splitter_thickness).max(160.0);

            let left_rect =
                egui::Rect::from_min_size(total_rect.min, egui::vec2(left_w, total_rect.height()));
            let v_split_rect = egui::Rect::from_min_size(
                egui::pos2(split_x, total_rect.top()),
                egui::vec2(splitter_thickness, total_rect.height()),
            );
            let right_rect = egui::Rect::from_min_size(
                egui::pos2(split_x + splitter_thickness, total_rect.top()),
                egui::vec2(right_w, total_rect.height()),
            );

            // 左エリア描画（固有の id_salt を付与してウィジェット ID 衝突を防止）
            ui.allocate_new_ui(
                egui::UiBuilder::new().id_salt("left_area").max_rect(left_rect),
                |ui| {
                    render_left_area(ui, state, theme, &mut actions);
                },
            );

            // 垂直スプリッタ操作
            let v_resp = ui.interact(v_split_rect, ui.id().with("main_v_splitter"), Sense::drag());
            ui.painter().rect_filled(v_split_rect, 0.0, theme.splitter_bg);
            if v_resp.hovered() || v_resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if v_resp.dragged() && total_rect.width() > 0.0 {
                let delta = v_resp.drag_delta().x / total_rect.width();
                state.window_state.left_panel_ratio =
                    (state.window_state.left_panel_ratio + delta).clamp(0.35, 0.85);
            }

            // 右エリア描画
            ui.allocate_new_ui(
                egui::UiBuilder::new().id_salt("right_area").max_rect(right_rect),
                |ui| {
                    render_right_area(ui, state, theme, &mut actions);
                },
            );
        } else {
            render_left_area(ui, state, theme, &mut actions);
        }
    });

    // 6. モーダル（接続ダイアログ・安全確認モーダル・バージョン情報ダイアログ）
    render_login_dialog(ctx, state, theme, &mut actions);
    render_confirm_modal(ctx, state, &mut actions);
    render_version_dialog(ctx, state);

    actions
}

/// 左側上下ペイン（上：SQL タブエディタ / 下：ログ・結果グリッド）を描画する。
fn render_left_area(
    ui: &mut Ui,
    state: &mut UiState,
    theme: &AppTheme,
    actions: &mut Vec<AppAction>,
) {
    let rect = ui.available_rect_before_wrap();
    let splitter_h = 5.0;
    let top_h = (rect.height() * state.window_state.left_split_ratio)
        .clamp(120.0, (rect.height() - 120.0).max(120.0));
    let bottom_h = (rect.height() - top_h - splitter_h).max(100.0);

    let top_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), top_h));
    let split_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.top() + top_h),
        egui::vec2(rect.width(), splitter_h),
    );
    let bottom_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.top() + top_h + splitter_h),
        egui::vec2(rect.width(), bottom_h),
    );

    // 左上ペイン：タブバー＋SQL エディタ
    ui.allocate_new_ui(
        egui::UiBuilder::new().id_salt("left_top_pane").max_rect(top_rect),
        |ui| {
            ui.horizontal(|ui| {
                let tab_count = state.tabs.len();
                let mut switch_to = None;
                let mut close_idx = None;
                for (idx, tab) in state.tabs.iter().enumerate() {
                    let label = if tab.dirty {
                        format!("{} *", tab.title)
                    } else {
                        tab.title.clone()
                    };
                    if ui.selectable_label(state.active_tab == idx, label).clicked() {
                        switch_to = Some(idx);
                    }
                    if tab_count > 1 && ui.small_button("×").clicked() {
                        close_idx = Some(idx);
                    }
                }
                if ui.small_button("+").clicked() {
                    state.add_new_tab();
                }
                if let Some(i) = switch_to {
                    state.active_tab = i;
                }
                if let Some(i) = close_idx {
                    state.close_tab(i);
                }
            });
            ui.separator();

            let font_size = state.config.font_size;
            let active_idx = state.active_tab;
            if active_idx < state.tabs.len() {
                let mut find_replace = std::mem::take(&mut state.find_replace);
                render_editor_pane(
                    ui,
                    &mut state.tabs[active_idx],
                    &mut find_replace,
                    theme,
                    font_size,
                );
                state.find_replace = find_replace;
            }
        },
    );

    // 左上下スプリッタ
    let h_resp = ui.interact(split_rect, ui.id().with("left_h_splitter"), Sense::drag());
    ui.painter().rect_filled(split_rect, 0.0, theme.splitter_bg);
    if h_resp.hovered() || h_resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if h_resp.dragged() && rect.height() > 0.0 {
        let delta = h_resp.drag_delta().y / rect.height();
        state.window_state.left_split_ratio =
            (state.window_state.left_split_ratio + delta).clamp(0.2, 0.85);
    }

    // 左下ペイン：ログ / 結果グリッド
    ui.allocate_new_ui(
        egui::UiBuilder::new().id_salt("left_bottom_pane").max_rect(bottom_rect),
        |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.bottom_tab, BottomTab::Log, "ログ");
                let grid_label = match &state.result {
                    Some(rs) => format!("結果グリッド ({}件)", rs.row_count),
                    None => "結果グリッド".to_string(),
                };
                ui.selectable_value(&mut state.bottom_tab, BottomTab::Grid, grid_label);
            });
            ui.separator();

            match state.bottom_tab {
                BottomTab::Log => render_log_pane(ui, state, theme),
                BottomTab::Grid => render_result_grid(ui, state, theme, actions),
            }
        },
    );
}

/// 左下ペインの実行ログタブを描画する。
fn render_log_pane(ui: &mut Ui, state: &UiState, theme: &AppTheme) {
    egui::ScrollArea::both()
        .id_salt("query_log_scroll")
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for entry in &state.logs {
                ui.label(
                    RichText::new(format!("-- {}", entry.timestamp))
                        .monospace()
                        .color(theme.log_timestamp),
                );
                ui.label(RichText::new(&entry.sql).monospace());
                let summary_color = if entry.is_error {
                    theme.log_error
                } else {
                    theme.identifier
                };
                ui.label(
                    RichText::new(&entry.summary)
                        .monospace()
                        .color(summary_color),
                );
                ui.label(
                    RichText::new("------------------------------------------------------------")
                        .monospace()
                        .color(theme.null_fg),
                );
            }
        });
}

/// 右側上下ペイン（上：オブジェクト一覧 / 下：カラム詳細）を描画する。
fn render_right_area(
    ui: &mut Ui,
    state: &mut UiState,
    theme: &AppTheme,
    actions: &mut Vec<AppAction>,
) {
    let rect = ui.available_rect_before_wrap();
    let splitter_h = 5.0;
    let top_h = (rect.height() * state.window_state.right_split_ratio)
        .clamp(120.0, (rect.height() - 120.0).max(120.0));
    let bottom_h = (rect.height() - top_h - splitter_h).max(100.0);

    let top_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), top_h));
    let split_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.top() + top_h),
        egui::vec2(rect.width(), splitter_h),
    );
    let bottom_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.top() + top_h + splitter_h),
        egui::vec2(rect.width(), bottom_h),
    );

    ui.allocate_new_ui(
        egui::UiBuilder::new().id_salt("right_top_pane").max_rect(top_rect),
        |ui| {
            render_object_list_pane(ui, state, theme, actions);
        },
    );

    let h_resp = ui.interact(split_rect, ui.id().with("right_h_splitter"), Sense::drag());
    ui.painter().rect_filled(split_rect, 0.0, theme.splitter_bg);
    if h_resp.hovered() || h_resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if h_resp.dragged() && rect.height() > 0.0 {
        let delta = h_resp.drag_delta().y / rect.height();
        state.window_state.right_split_ratio =
            (state.window_state.right_split_ratio + delta).clamp(0.2, 0.85);
    }

    ui.allocate_new_ui(
        egui::UiBuilder::new().id_salt("right_bottom_pane").max_rect(bottom_rect),
        |ui| {
            render_object_detail_pane(ui, state, theme);
        },
    );
}

/// グローバルショートカットキー（F5 / Ctrl+Enter / Ctrl+F / Ctrl+H / Ctrl+N/O/S / F8）を処理する。
fn handle_shortcuts(ctx: &egui::Context, state: &mut UiState, actions: &mut Vec<AppAction>) {
    if state.confirm_modal.is_some() || state.login_dialog.open || state.show_version_dialog {
        return;
    }

    let (exec_pressed, find_pressed, replace_pressed, new_tab, open_file, save_file, toggle_browser) =
        ctx.input_mut(|i| {
            let exec = i.key_pressed(Key::F5) || i.consume_key(Modifiers::CTRL, Key::Enter);
            let find = i.consume_key(Modifiers::CTRL, Key::F);
            let repl = i.consume_key(Modifiers::CTRL, Key::H);
            let new_t = i.consume_key(Modifiers::CTRL, Key::N);
            let open_f = i.consume_key(Modifiers::CTRL, Key::O);
            let save_f = i.consume_key(Modifiers::CTRL, Key::S);
            let tog_b = i.key_pressed(Key::F8);
            (exec, find, repl, new_t, open_f, save_f, tog_b)
        });

    if exec_pressed && state.active_profile.is_some() && !state.busy {
        request_execute_current_sql(state, actions);
    }
    if find_pressed {
        state.find_replace.open = true;
        state.find_replace.show_replace = false;
    }
    if replace_pressed {
        state.find_replace.open = true;
        state.find_replace.show_replace = true;
    }
    if new_tab {
        state.add_new_tab();
    }
    if open_file {
        open_sql_file(state);
    }
    if save_file {
        save_active_tab(state, false);
    }
    if toggle_browser {
        state.window_state.show_browser = !state.window_state.show_browser;
    }
}

/// 選択範囲またはカーソル位置の SQL 文を抽出し、各種安全ガードを通した上で実行要求を発行する。
fn request_execute_current_sql(state: &mut UiState, actions: &mut Vec<AppAction>) {
    let Some(tab) = state.active_tab_ref() else {
        return;
    };

    let sql_to_run = if let Some((a, b)) = tab.selection_byte_range {
        tab.text[a.min(tab.text.len())..b.min(tab.text.len())]
            .trim()
            .to_string()
    } else if let Some(stmt) = statement_at_offset(&tab.text, tab.cursor_byte_offset) {
        stmt.sql.to_string()
    } else {
        return;
    };

    if sql_to_run.is_empty() {
        return;
    }

    let kind = analyze_statement(&sql_to_run);
    let is_prod = state.is_production();
    let confirm_dml_ddl = state
        .active_profile
        .as_ref()
        .is_some_and(|p| p.confirm_on_dml_ddl);
    let confirm_commit = state
        .active_profile
        .as_ref()
        .is_some_and(|p| p.confirm_on_commit);

    // 読み取り専用セッション中の更新系 SQL ブロック
    if state.read_only_session
        && matches!(
            kind,
            StatementKind::Dml | StatementKind::Ddl | StatementKind::Commit
        )
    {
        state.push_log(
            sql_to_run,
            "エラー: 読み取り専用モードのため更新系 SQL はブロックされました。".to_string(),
            true,
            None,
        );
        state.bottom_tab = BottomTab::Log;
        return;
    }

    // COMMIT 文の確認ガード
    if kind == StatementKind::Commit && (is_prod || confirm_commit) {
        state.confirm_modal = Some(ConfirmModal {
            title: "コミットの確認".to_string(),
            message: if is_prod {
                format!(
                    "本番環境です。未コミットの変更（{}件）を本当にコミットしますか？",
                    state.uncommitted_count
                )
            } else {
                format!(
                    "未コミットの変更（{}件）をコミットしますか？",
                    state.uncommitted_count
                )
            },
            sql_preview: Some(sql_to_run),
            is_danger: is_prod,
            confirm_label: "コミットを実行".to_string(),
            action: ConfirmAction::ProductionCommit {
                uncommitted_count: state.uncommitted_count,
            },
        });
        return;
    }

    // WHERE 句なし UPDATE / DELETE の全件更新・削除ガード
    if kind == StatementKind::Dml
        && state.config.confirm_where_less_dml
        && is_dml_without_where(&sql_to_run)
    {
        state.confirm_modal = Some(ConfirmModal {
            title: "WHERE 句なしの更新/削除を検知".to_string(),
            message: if is_prod {
                "⚠ 本番環境で WHERE 句のない UPDATE / DELETE を実行しようとしています！全件が更新・削除される可能性があります。実行しますか？"
            } else {
                "WHERE 句のない UPDATE / DELETE を実行しようとしています。全件が対象になる可能性があります。実行しますか？"
            }
            .to_string(),
            sql_preview: Some(sql_to_run.clone()),
            is_danger: true,
            confirm_label: "このまま実行".to_string(),
            action: ConfirmAction::DmlWithoutWhere { sql: sql_to_run },
        });
        return;
    }

    // 未コミット変更が残っている状態での DDL 実行（暗黙コミット警告）
    if kind == StatementKind::Ddl && state.uncommitted_count > 0 {
        state.confirm_modal = Some(ConfirmModal {
            title: "DDL 実行による暗黙コミットの警告".to_string(),
            message: format!(
                "未コミットの変更が {} 件残っています。Oracle 等では DDL 実行時に直前までの変更が自動的にコミットされます。続行しますか？",
                state.uncommitted_count
            ),
            sql_preview: Some(sql_to_run.clone()),
            is_danger: is_prod,
            confirm_label: "DDL を実行".to_string(),
            action: ConfirmAction::ImplicitCommitDdl {
                sql: sql_to_run,
                uncommitted_count: state.uncommitted_count,
            },
        });
        return;
    }

    // 本番環境または確認有効環境での DML / DDL 実行前確認
    if matches!(kind, StatementKind::Dml | StatementKind::Ddl) && (is_prod || confirm_dml_ddl) {
        let is_ddl = kind == StatementKind::Ddl;
        state.confirm_modal = Some(ConfirmModal {
            title: if is_prod {
                "本番環境でのデータ更新・定義変更の確認".to_string()
            } else {
                "検証環境での更新・定義変更の確認".to_string()
            },
            message: if is_prod {
                "本番環境に対して更新系 SQL を実行しようとしています。内容を確認してください。"
            } else {
                "更新系 SQL を実行しようとしています。続行しますか？"
            }
            .to_string(),
            sql_preview: Some(sql_to_run.clone()),
            is_danger: is_prod,
            confirm_label: "実行する".to_string(),
            action: ConfirmAction::ProductionSql {
                sql: sql_to_run,
                is_ddl,
                has_uncommitted: state.uncommitted_count > 0,
                uncommitted_count: state.uncommitted_count,
            },
        });
        return;
    }

    actions.push(AppAction::ExecuteSql {
        sql: sql_to_run,
        allow_write: !state.read_only_session,
    });
}

/// ツールバーやメニューからのコミット押下時に確認ガードを通す。
fn request_commit(state: &mut UiState, actions: &mut Vec<AppAction>) {
    let is_prod = state.is_production();
    let confirm_commit = state
        .active_profile
        .as_ref()
        .is_some_and(|p| p.confirm_on_commit);

    if is_prod || confirm_commit {
        state.confirm_modal = Some(ConfirmModal {
            title: "コミットの確認".to_string(),
            message: if is_prod {
                format!(
                    "⚠ 本番環境です。未コミットの変更（{}件）を本当にコミットしますか？",
                    state.uncommitted_count
                )
            } else {
                format!(
                    "未コミットの変更（{}件）をコミットしますか？",
                    state.uncommitted_count
                )
            },
            sql_preview: Some("COMMIT;".to_string()),
            is_danger: is_prod,
            confirm_label: "コミットを実行".to_string(),
            action: ConfirmAction::ProductionCommit {
                uncommitted_count: state.uncommitted_count,
            },
        });
    } else {
        actions.push(AppAction::Commit);
    }
}

/// SQL ファイル選択ダイアログを開き、新しいタブとして読み込む。
fn open_sql_file(state: &mut UiState) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("SQL ファイル", &["sql", "txt"])
        .pick_file()
    else {
        return;
    };

    if let Ok(bytes) = fs::read(&path) {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let id = state.next_tab_id;
        state.next_tab_id += 1;
        let title = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Query.sql")
            .to_string();
        let mut tab = SqlTab::new(id, title, text);
        tab.file_path = Some(path);
        state.tabs.push(tab);
        state.active_tab = state.tabs.len() - 1;
    }
}

/// アクティブな SQL タブの内容をファイルへ保存する。
fn save_active_tab(state: &mut UiState, force_dialog: bool) {
    let Some(tab) = state.active_tab_mut() else {
        return;
    };

    let target = if force_dialog || tab.file_path.is_none() {
        rfd::FileDialog::new()
            .add_filter("SQL ファイル", &["sql", "txt"])
            .set_file_name(&tab.title)
            .save_file()
    } else {
        tab.file_path.clone()
    };

    if let Some(path) = target
        && fs::write(&path, tab.text.as_bytes()).is_ok()
    {
        tab.title = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&tab.title)
            .to_string();
        tab.file_path = Some(path);
        tab.dirty = false;
        state.status_notice = Some("ファイルを保存しました。".to_string());
    }
}
