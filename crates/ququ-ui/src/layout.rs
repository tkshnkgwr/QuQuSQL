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
                if let Some(stats) = state.active_selection_stats() {
                    ui.separator();
                    let text = if stats.numeric_count > 0 {
                        let avg = stats.avg.unwrap_or(0.0);
                        let min = stats.min.unwrap_or(0.0);
                        let max = stats.max.unwrap_or(0.0);
                        format!(
                            "選択: {}セル (非NULL:{}) | 数値:{} | 合計:{} | 平均:{} | 最小:{} | 最大:{}",
                            stats.cell_count,
                            stats.non_null_count,
                            stats.numeric_count,
                            format_stat_num(stats.sum),
                            format_stat_num(avg),
                            format_stat_num(min),
                            format_stat_num(max),
                        )
                    } else {
                        format!(
                            "選択: {}セル (非NULL: {})",
                            stats.cell_count, stats.non_null_count
                        )
                    };
                    // 末尾の .0000 だけの整数見栄えを軽く調整する代わりにシンプルに表示
                    ui.label(RichText::new(text).small().color(Color32::from_rgb(0, 102, 180)));
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

    // 左上ペイン：Zed 風タブバー（横移動ボタン・各タブ閉じるボタン）＋SQL エディタ
    ui.allocate_new_ui(
        egui::UiBuilder::new().id_salt("left_top_pane").max_rect(top_rect),
        |ui| {
            render_sql_tab_bar(ui, state, theme);
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

/// Zed 風の SQL タブバー（左右移動ボタン・横スクロール・各タブ閉じるボタン）を描画する。
fn render_sql_tab_bar(ui: &mut Ui, state: &mut UiState, theme: &AppTheme) {
    let tab_count = state.tabs.len();
    let mut switch_to = None;
    let mut close_idx = None;
    let mut scroll_delta = 0.0_f32;
    let mut add_tab = false;

    let bar_bg = if theme.dark_mode {
        Color32::from_rgb(36, 39, 46)
    } else {
        Color32::from_rgb(236, 238, 242)
    };
    let inactive_bg = if theme.dark_mode {
        Color32::from_rgb(42, 46, 54)
    } else {
        Color32::from_rgb(228, 231, 236)
    };
    let border_color = theme.splitter_bg;
    let accent_color = Color32::from_rgb(56, 132, 244);

    egui::Frame::NONE
        .fill(bar_bg)
        .inner_margin(egui::Margin::symmetric(2, 2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;

                // Zed 風の左端横移動ボタン（← / →）
                let nav_btn_size = egui::vec2(22.0, 24.0);
                let left_btn = egui::Button::new(RichText::new("←").small())
                    .min_size(nav_btn_size)
                    .fill(inactive_bg);
                if ui
                    .add(left_btn)
                    .on_hover_text("タブを左へスクロール")
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    scroll_delta -= 160.0;
                }

                let right_btn = egui::Button::new(RichText::new("→").small())
                    .min_size(nav_btn_size)
                    .fill(inactive_bg);
                if ui
                    .add(right_btn)
                    .on_hover_text("タブを右へスクロール")
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    scroll_delta += 160.0;
                }

                // 右端の「+」ボタン分を除いた中央領域で横スクロールする
                let scroll_width = (ui.available_width() - 28.0).max(40.0);
                ui.allocate_ui(egui::vec2(scroll_width, 26.0), |ui| {
                    let target_offset = (state.tab_scroll_offset + scroll_delta).max(0.0);
                    let scroll_out = egui::ScrollArea::horizontal()
                        .id_salt("sql_tab_bar_scroll")
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                        )
                        .scroll_offset(egui::vec2(target_offset, 0.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                for (idx, tab) in state.tabs.iter().enumerate() {
                                    let is_active = state.active_tab == idx;
                                    let bg = if is_active {
                                        theme.editor_bg
                                    } else {
                                        inactive_bg
                                    };
                                    let title = if tab.dirty {
                                        format!("{} *", tab.title)
                                    } else {
                                        tab.title.clone()
                                    };

                                    let frame_resp = egui::Frame::NONE
                                        .fill(bg)
                                        .stroke(egui::Stroke::new(1.0_f32, border_color))
                                        .inner_margin(egui::Margin::symmetric(8, 3))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 6.0;
                                                let text_color = if is_active {
                                                    theme.identifier
                                                } else {
                                                    theme.null_fg
                                                };
                                                let lbl = ui
                                                    .add(
                                                        egui::Label::new(
                                                            RichText::new(title)
                                                                .small()
                                                                .color(text_color),
                                                        )
                                                        .selectable(false)
                                                        .sense(Sense::click()),
                                                    )
                                                    .on_hover_cursor(
                                                        egui::CursorIcon::PointingHand,
                                                    );
                                                if lbl.clicked() {
                                                    switch_to = Some(idx);
                                                }

                                                // Zed 風の各タブ閉じるボタン（×）
                                                let close_btn = egui::Button::new(
                                                    RichText::new("×").small().color(text_color),
                                                )
                                                .frame(false);
                                                if ui
                                                    .add_enabled(tab_count > 1, close_btn)
                                                    .on_hover_text("タブを閉じる")
                                                    .on_hover_cursor(
                                                        egui::CursorIcon::PointingHand,
                                                    )
                                                    .clicked()
                                                {
                                                    close_idx = Some(idx);
                                                }
                                            });
                                        });

                                    // アクティブタブの上辺にアクセントラインを描画する
                                    if is_active {
                                        let r = frame_resp.response.rect;
                                        ui.painter().line_segment(
                                            [
                                                egui::pos2(r.left(), r.top() + 1.0),
                                                egui::pos2(r.right(), r.top() + 1.0),
                                            ],
                                            egui::Stroke::new(2.0_f32, accent_color),
                                        );
                                    }
                                }
                            });
                        });
                    let max_scroll =
                        (scroll_out.content_size.x - scroll_out.inner_rect.width()).max(0.0);
                    state.tab_scroll_offset = scroll_out.state.offset.x.clamp(0.0, max_scroll);
                });

                if ui
                    .add(
                        egui::Button::new(RichText::new("+").small())
                            .min_size(nav_btn_size)
                            .fill(inactive_bg),
                    )
                    .on_hover_text("新しい SQL タブ")
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    add_tab = true;
                }
            });
        });

    if let Some(i) = switch_to {
        state.active_tab = i;
    }
    if let Some(i) = close_idx {
        state.close_tab(i);
    }
    if add_tab {
        state.add_new_tab();
        // 新規タブが見切れないよう右端方向へ少し寄せる
        state.tab_scroll_offset += 200.0;
    }
}

/// 左下ペインの実行ログタブを描画する（右クリックでログのコピー・クリアに対応）。
fn render_log_pane(ui: &mut Ui, state: &mut UiState, theme: &AppTheme) {
    let mut copy_logs = false;
    let mut clear_logs = false;
    let log_rect = ui.available_rect_before_wrap();

    egui::ScrollArea::both()
        .id_salt("query_log_scroll")
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if state.logs.is_empty() {
                ui.label(
                    RichText::new("実行ログはまだありません。（右クリックでメニュー表示）")
                        .small()
                        .color(theme.null_fg),
                );
            } else {
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
                        RichText::new(
                            "------------------------------------------------------------",
                        )
                        .monospace()
                        .color(theme.null_fg),
                    );
                }
            }
        });

    // ペイン全体（右側の余白を含む）のどこを右クリックしてもメニューを開く
    let resp = ui.interact(
        log_rect,
        ui.id().with("log_pane_context_area"),
        Sense::click(),
    );
    resp.context_menu(|ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        if ui
            .add_enabled(!state.logs.is_empty(), egui::Button::new("ログのコピー"))
            .clicked()
        {
            copy_logs = true;
            ui.close_menu();
        }
        if ui
            .add_enabled(!state.logs.is_empty(), egui::Button::new("ログのクリア"))
            .clicked()
        {
            clear_logs = true;
            ui.close_menu();
        }
    });

    if copy_logs {
        let text = format_logs_text(&state.logs);
        ui.ctx().copy_text(text);
        state.status_notice = Some("実行ログをクリップボードへコピーしました。".to_string());
    }
    if clear_logs {
        state.logs.clear();
        state.status_notice = Some("実行ログをクリアしました。".to_string());
    }
}

/// 実行ログ一覧をクリップボードコピー用のテキストへ整形する。
fn format_logs_text(logs: &[ququ_core::QueryLogEntry]) -> String {
    let mut out = String::new();
    for entry in logs {
        out.push_str(&format!("-- {}\n", entry.timestamp));
        out.push_str(&entry.sql);
        if !entry.sql.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&entry.summary);
        out.push_str("\n------------------------------------------------------------\n");
    }
    out
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

/// 選択統計の浮動小数点数を小数点以下最大 4 桁で整形し、末尾の冗長なゼロを省く。
fn format_stat_num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 && v.abs() < 1e15 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_logs_for_clipboard_copy() {
        let logs = vec![ququ_core::QueryLogEntry {
            timestamp: "2026/11/27 17:00:00".to_string(),
            sql: "SELECT 1".to_string(),
            summary: "1 件選択されました".to_string(),
            is_error: false,
            elapsed_ms: Some(1),
        }];
        let txt = format_logs_text(&logs);
        assert!(txt.contains("-- 2026/11/27 17:00:00"));
        assert!(txt.contains("SELECT 1"));
        assert!(txt.contains("1 件選択されました"));
    }
}
