//! 仮想スクロール対応の結果グリッドおよびテーブル直接編集 UI モジュール。
//!
//! SQL 実行結果（読み取り専用 `ResultSet`）の表示に加えて、
//! テーブル単位でのセル直接編集・行挿入/複製/削除マーク・Undo/Redo・
//! Excel 互換 TSV コピー＆ペースト・CSV/TSV/INSERT エクスポートを提供する。

use crate::state::{AppAction, ConfirmAction, ConfirmModal, GridEditorState, UiState};
use crate::theme::AppTheme;
use egui::{Align, Color32, RichText, Sense, Stroke, StrokeKind, Ui};
use egui_extras::{Column, TableBuilder};
use ququ_core::{
    CellValue, ColumnMeta, DisplayRowKind, GridEditOp, ParameterizedDml, ResultSet, SqlDialect,
    export_csv, export_csv_with_header, export_insert_sql, export_tsv, parse_cell_input,
    parse_tsv,
};
use ququ_sql::{format_dml_preview_script, generate_grid_dml};
use std::fs;

/// グリッド上のポインタ操作から生成される複数セル範囲選択イベント。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionEvent {
    /// 左ボタン押下で単一セル選択／ドラッグ開始（Shift 押下時はアンカー維持）。
    StartCell(usize, usize),
    /// ドラッグ中に別セル上へポインタが入った際の終点追従。
    DragHover(usize, usize),
    /// 右クリック時（既存の選択範囲外なら単一セルへ切り替え、範囲内なら維持）。
    RightClick(usize, usize),
    /// 行番号 `#` ヘッダクリックによる行全体選択。
    SelectRow(usize),
    /// 列ヘッダクリックによる列全体選択。
    SelectCol(usize),
    /// 左上 `#` クリックによる全セル選択。
    SelectAll,
}

/// 選択始点・終点から昇順の矩形 `(min_row, max_row, min_col, max_col)` を計算する。
fn rect_from_points(
    anchor: Option<(usize, usize)>,
    focus: Option<(usize, usize)>,
) -> Option<(usize, usize, usize, usize)> {
    let f = focus?;
    let a = anchor.unwrap_or(f);
    Some((a.0.min(f.0), a.0.max(f.0), a.1.min(f.1), a.1.max(f.1)))
}

/// 範囲選択イベントを選択状態へ反映する（読み取り専用・編集グリッド共通）。
fn apply_selection_event(
    anchor: &mut Option<(usize, usize)>,
    focus: &mut Option<(usize, usize)>,
    is_selecting: &mut bool,
    row_count: usize,
    col_count: usize,
    shift_held: bool,
    event: SelectionEvent,
) {
    match event {
        SelectionEvent::StartCell(r, c) => {
            if shift_held && anchor.is_some() {
                *focus = Some((r, c));
            } else {
                *anchor = Some((r, c));
                *focus = Some((r, c));
            }
            *is_selecting = true;
        }
        SelectionEvent::DragHover(r, c) => {
            *focus = Some((r, c));
        }
        SelectionEvent::RightClick(r, c) => {
            let inside = rect_from_points(*anchor, *focus)
                .is_some_and(|(r0, r1, c0, c1)| r >= r0 && r <= r1 && c >= c0 && c <= c1);
            if !inside {
                *anchor = Some((r, c));
                *focus = Some((r, c));
            }
        }
        SelectionEvent::SelectRow(r) => {
            if col_count == 0 {
                return;
            }
            if shift_held && let Some(prev) = *anchor {
                *anchor = Some((prev.0, 0));
                *focus = Some((r, col_count - 1));
            } else {
                *anchor = Some((r, 0));
                *focus = Some((r, col_count - 1));
            }
        }
        SelectionEvent::SelectCol(c) => {
            if row_count == 0 {
                return;
            }
            if shift_held && let Some(prev) = *anchor {
                *anchor = Some((0, prev.1));
                *focus = Some((row_count - 1, c));
            } else {
                *anchor = Some((0, c));
                *focus = Some((row_count - 1, c));
            }
        }
        SelectionEvent::SelectAll => {
            if row_count > 0 && col_count > 0 {
                *anchor = Some((0, 0));
                *focus = Some((row_count - 1, col_count - 1));
            }
        }
    }
}

/// 下部ペインのグリッドタブを描画する（テーブル編集中は編集グリッドを優先表示）。
pub fn render_result_grid(
    ui: &mut Ui,
    state: &mut UiState,
    theme: &AppTheme,
    actions: &mut Vec<AppAction>,
) {
    if state.grid_editor.is_some() {
        render_grid_editor(ui, state, theme, actions);
    } else {
        render_readonly_result_grid(ui, state, theme, actions);
    }
}

/// 通常の SQL 実行結果（読み取り専用 ResultSet）グリッドを描画する。
fn render_readonly_result_grid(
    ui: &mut Ui,
    state: &mut UiState,
    theme: &AppTheme,
    actions: &mut Vec<AppAction>,
) {
    let Some(rs) = state.result.as_ref() else {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new(
                    "SELECT クエリを実行するか、左ペインのテーブルを右クリックして「グリッドで開く」を選択してください。",
                )
                .color(theme.null_fg),
            );
        });
        return;
    };

    let mut notice: Option<String> = None;

    // グリッド上部バー（件数・実行時間・追加フェッチ・エクスポート）
    ui.horizontal(|ui| {
        let more_text = if rs.has_more { "（未取得あり）" } else { "" };
        ui.label(
            RichText::new(format!(
                "{} 行 × {} 列{} | {} ms",
                rs.row_count,
                rs.col_count(),
                more_text,
                rs.elapsed_ms
            ))
            .strong(),
        );

        if rs.has_more
            && let Some(src_sql) = rs.source_sql.clone()
        {
            if ui.button("+ さらに 1,000 行").clicked() && !state.busy {
                actions.push(AppAction::FetchMore {
                    sql: src_sql.clone(),
                    offset: rs.row_count,
                    limit: state.config.default_fetch_limit,
                });
            }
            if ui.button("全件取得").clicked() && !state.busy {
                actions.push(AppAction::FetchMore {
                    sql: src_sql,
                    offset: rs.row_count,
                    limit: 0,
                });
            }
        }

        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            let dialect = active_dialect(state);
            ui.menu_button("エクスポート", |ui| {
                if ui.button("CSV をファイル保存...").clicked() {
                    ui.close_menu();
                    let content = export_result_csv(rs);
                    notice = save_export_dialog("query_result.csv", "CSV", &["csv"], &content);
                }
                if ui.button("TSV をファイル保存...").clicked() {
                    ui.close_menu();
                    let content = result_set_to_tsv(rs, true);
                    notice = save_export_dialog("query_result.tsv", "TSV", &["tsv"], &content);
                }
                if ui.button("INSERT 文をファイル保存...").clicked() {
                    ui.close_menu();
                    let content = export_result_insert_sql(rs, "TARGET_TABLE", dialect);
                    notice = save_export_dialog("insert_data.sql", "SQL", &["sql"], &content);
                }
                ui.separator();
                if ui.button("TSV コピー (全行)").clicked() {
                    ui.ctx().copy_text(result_set_to_tsv(rs, true));
                    notice = Some("結果グリッドを TSV 形式でコピーしました。".to_string());
                    ui.close_menu();
                }
                if ui.button("CSV コピー (全行)").clicked() {
                    ui.ctx().copy_text(export_result_csv(rs));
                    notice = Some("結果グリッドを CSV 形式でコピーしました。".to_string());
                    ui.close_menu();
                }
                if ui.button("INSERT 文コピー (全行)").clicked() {
                    ui.ctx()
                        .copy_text(export_result_insert_sql(rs, "TARGET_TABLE", dialect));
                    notice = Some("全行の INSERT 文をコピーしました。".to_string());
                    ui.close_menu();
                }
            });
        });
    });

    ui.separator();

    if rs.columns.is_empty() {
        ui.label("表示する列がありません。");
        if let Some(n) = notice {
            state.status_notice = Some(n);
        }
        return;
    }

    let (primary_pressed, primary_down, shift_held, pointer_pos) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.modifiers.shift,
            i.pointer.interact_pos(),
        )
    });
    if !primary_down {
        state.grid_selecting = false;
    }

    let row_height = 22.0;
    let col_count = rs.col_count();

    let mut table = TableBuilder::new(ui)
        .id_salt("query_result_table")
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(Align::Center))
        .column(Column::exact(48.0));

    for _ in 0..col_count {
        table = table.column(Column::initial(130.0).at_least(50.0).clip(true));
    }

    let mut cell_buf = String::with_capacity(64);
    let mut selection_event: Option<SelectionEvent> = None;
    let mut copy_text: Option<String> = None;
    let sel_rect = rect_from_points(state.selection_anchor, state.selected_cell);

    table
        .header(24.0, |mut header| {
            header.col(|ui| {
                ui.add(egui::Label::new(RichText::new("#").strong()).selectable(false));
                let resp = ui
                    .interact(
                        ui.max_rect(),
                        ui.id().with("ro_header_all"),
                        Sense::click(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    selection_event = Some(SelectionEvent::SelectAll);
                }
            });
            for (col_idx, col) in rs.columns.iter().enumerate() {
                header.col(|ui| {
                    ui.add(
                        egui::Label::new(RichText::new(col.name.as_str()).strong())
                            .selectable(false),
                    );
                    let mut resp = ui
                        .interact(
                            ui.max_rect(),
                            ui.id().with(("ro_col_hdr", col_idx)),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if !col.data_type.is_empty() {
                        resp = resp.on_hover_text(col.data_type.as_str());
                    }
                    if resp.clicked() {
                        selection_event = Some(SelectionEvent::SelectCol(col_idx));
                    }
                });
            }
        })
        .body(|body| {
            body.rows(row_height, rs.row_count, |mut row| {
                let row_idx = row.index();
                row.col(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("{}", row_idx + 1))
                                    .small()
                                    .color(theme.editor_gutter_fg),
                            )
                            .selectable(false),
                        );
                    });
                    let resp = ui
                        .interact(
                            ui.max_rect(),
                            ui.id().with(("ro_row_hdr", row_idx)),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if resp.clicked() {
                        selection_event = Some(SelectionEvent::SelectRow(row_idx));
                    }
                });

                for col_idx in 0..col_count {
                    row.col(|ui| {
                        let Some(val) = rs.get(row_idx, col_idx) else {
                            return;
                        };
                        let rect = ui.max_rect();
                        let is_selected = sel_rect.is_some_and(|(r0, r1, c0, c1)| {
                            row_idx >= r0 && row_idx <= r1 && col_idx >= c0 && col_idx <= c1
                        });
                        if is_selected {
                            ui.painter().rect_filled(rect, 0.0, theme.grid_selected_bg);
                            ui.painter().rect_stroke(
                                rect.shrink(0.5),
                                0.0,
                                Stroke::new(1.5_f32, Color32::from_rgb(45, 125, 225)),
                                StrokeKind::Inside,
                            );
                        }

                        val.write_display_into(&mut cell_buf);
                        let rich = if val.is_null() {
                            RichText::new(&cell_buf).italics().color(theme.null_fg)
                        } else {
                            RichText::new(&cell_buf)
                        };

                        if val.is_numeric() {
                            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                ui.add(egui::Label::new(rich).selectable(false));
                            });
                        } else {
                            ui.add(egui::Label::new(rich).selectable(false));
                        }

                        let resp = ui.interact(
                            rect,
                            ui.id().with(("grid_cell", row_idx, col_idx)),
                            Sense::click_and_drag(),
                        );
                        if primary_pressed && resp.hovered() {
                            selection_event = Some(SelectionEvent::StartCell(row_idx, col_idx));
                        } else if state.grid_selecting
                            && primary_down
                            && pointer_pos
                                .is_some_and(|p| rect.contains(p) && ui.clip_rect().contains(p))
                        {
                            selection_event = Some(SelectionEvent::DragHover(row_idx, col_idx));
                        }
                        if resp.secondary_clicked() {
                            selection_event = Some(SelectionEvent::RightClick(row_idx, col_idx));
                        }
                        resp.context_menu(|ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                            if let Some((r0, r1, c0, c1)) = sel_rect {
                                if ui.button("選択範囲を TSV コピー").clicked() {
                                    copy_text =
                                        Some(copy_readonly_range(rs, r0, r1, c0, c1, false, false));
                                    ui.close_menu();
                                }
                                if ui.button("選択範囲を TSV コピー (カラム名付き)").clicked() {
                                    copy_text =
                                        Some(copy_readonly_range(rs, r0, r1, c0, c1, false, true));
                                    ui.close_menu();
                                }
                                if ui.button("選択範囲を CSV コピー").clicked() {
                                    copy_text =
                                        Some(copy_readonly_range(rs, r0, r1, c0, c1, true, false));
                                    ui.close_menu();
                                }
                                if ui.button("選択範囲を CSV コピー (カラム名付き)").clicked() {
                                    copy_text =
                                        Some(copy_readonly_range(rs, r0, r1, c0, c1, true, true));
                                    ui.close_menu();
                                }
                                ui.separator();
                            }
                            if ui.button("この行を TSV コピー").clicked() {
                                let line = (0..col_count)
                                    .map(|c| {
                                        rs.get(row_idx, c)
                                            .map(|v| v.copy_str().into_owned())
                                            .unwrap_or_default()
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\t");
                                copy_text = Some(line);
                                ui.close_menu();
                            }
                        });
                    });
                }
            });
        });

    // エディタにフォーカスが無い状態で Ctrl+C された場合、選択矩形を TSV コピーする
    if !ui.ctx().wants_keyboard_input()
        && let Some((r0, r1, c0, c1)) = sel_rect
    {
        let copy_pressed = ui.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Copy))
        });
        if copy_pressed {
            copy_text = Some(copy_readonly_range(rs, r0, r1, c0, c1, false, false));
        }
    }

    if let Some(ev) = selection_event {
        apply_selection_event(
            &mut state.selection_anchor,
            &mut state.selected_cell,
            &mut state.grid_selecting,
            rs.row_count,
            col_count,
            shift_held,
            ev,
        );
    }
    if let Some(txt) = copy_text {
        ui.ctx().copy_text(txt);
    }
    if let Some(n) = notice {
        state.status_notice = Some(n);
    }
}

/// テーブル直接編集モードのツールバー・WHERE/ORDER BY バー・仮想テーブルを描画する。
fn render_grid_editor(
    ui: &mut Ui,
    state: &mut UiState,
    theme: &AppTheme,
    actions: &mut Vec<AppAction>,
) {
    let Some(mut ge) = state.grid_editor.take() else {
        return;
    };
    let mut keep_editor = true;
    let mut notice: Option<String> = None;
    let mut pending_confirm: Option<ConfirmModal> = None;

    let display_rows = ge.patch.display_rows();
    let table_has_keys = ge.table.all_rows_editable() || ge.table.result.row_count == 0;
    let can_write_grid = !state.read_only_session && !state.busy && table_has_keys;

    let selected_focus = ge.selection_focus;
    let selected_logical_row = selected_focus.and_then(|(disp_r, _)| {
        display_rows.get(disp_r).map(|k| match *k {
            DisplayRowKind::Existing { logical_row, .. }
            | DisplayRowKind::Inserted { logical_row } => logical_row,
        })
    });
    let selected_is_deleted =
        selected_logical_row.is_some_and(|lr| ge.patch.is_row_deleted(lr));

    // 1. テーブル編集アクションツールバー
    ui.horizontal_wrapped(|ui| {
        let title = if ge.table.owner.trim().is_empty() {
            format!("テーブル編集: {}", ge.table.table_name)
        } else {
            format!("テーブル編集: {}.{}", ge.table.owner, ge.table.table_name)
        };
        ui.label(RichText::new(title).strong());

        let more_text = if ge.table.result.has_more {
            " (上限取得)"
        } else {
            ""
        };
        ui.label(
            RichText::new(format!(
                "{} 行 × {} 列{}",
                display_rows.len(),
                ge.table.columns.len(),
                more_text
            ))
            .small()
            .color(theme.null_fg),
        );

        if state.read_only_session {
            ui.label(
                RichText::new("[読み取り専用]")
                    .small()
                    .strong()
                    .color(Color32::from_rgb(200, 60, 40)),
            );
        } else if !table_has_keys {
            ui.label(
                RichText::new("[ROWIDなし・閲覧のみ]")
                    .small()
                    .strong()
                    .color(Color32::from_rgb(191, 100, 0)),
            );
        }

        ui.separator();

        if ui
            .add_enabled(can_write_grid, egui::Button::new("+ 行追加"))
            .clicked()
        {
            ge.editing_cell = None;
            let _new_lr = ge.patch.insert_row(&ge.table, None);
            let updated_rows = ge.patch.display_rows();
            if let Some(last_idx) = updated_rows.len().checked_sub(1) {
                ge.selection_anchor = Some((last_idx, 0));
                ge.selection_focus = Some((last_idx, 0));
            }
        }

        if ui
            .add_enabled(
                can_write_grid && selected_logical_row.is_some(),
                egui::Button::new("行複製"),
            )
            .clicked()
            && let Some(src_lr) = selected_logical_row
        {
            ge.editing_cell = None;
            ge.patch.duplicate_row(&ge.table, src_lr);
            let updated_rows = ge.patch.display_rows();
            if let Some(last_idx) = updated_rows.len().checked_sub(1) {
                ge.selection_anchor = Some((last_idx, 0));
                ge.selection_focus = Some((last_idx, 0));
            }
        }

        let del_label = if selected_is_deleted {
            "削除取消"
        } else {
            "行削除"
        };
        if ui
            .add_enabled(
                can_write_grid && selected_logical_row.is_some(),
                egui::Button::new(del_label),
            )
            .clicked()
            && let Some(lr) = selected_logical_row
        {
            ge.editing_cell = None;
            ge.patch.toggle_delete_row(&ge.table, lr);
        }

        if ui
            .add_enabled(
                can_write_grid && ge.patch.can_undo(),
                egui::Button::new("Undo"),
            )
            .clicked()
        {
            ge.editing_cell = None;
            ge.patch.undo(&ge.table);
        }

        if ui
            .add_enabled(
                can_write_grid && ge.patch.can_redo(),
                egui::Button::new("Redo"),
            )
            .clicked()
        {
            ge.editing_cell = None;
            ge.patch.redo(&ge.table);
        }

        if ui
            .add_enabled(
                ge.patch.has_changes() && !state.busy,
                egui::Button::new("変更破棄"),
            )
            .clicked()
        {
            ge.editing_cell = None;
            ge.patch.clear();
            notice = Some("未反映のグリッド変更を破棄しました。".to_string());
        }

        let has_changes = ge.patch.has_changes();
        let apply_text = if has_changes && can_write_grid {
            RichText::new("反映 (Apply)...").strong().color(Color32::WHITE)
        } else {
            RichText::new("反映 (Apply)...")
        };
        let mut apply_btn = egui::Button::new(apply_text);
        if has_changes && can_write_grid {
            apply_btn = apply_btn.fill(Color32::from_rgb(16, 124, 65));
        }
        if ui
            .add_enabled(has_changes && can_write_grid, apply_btn)
            .clicked()
        {
            ge.editing_cell = None;
            match generate_grid_dml(&ge.table, &ge.patch) {
                Ok(dmls) if !dmls.is_empty() => {
                    pending_confirm = Some(build_apply_confirm_modal(state.is_production(), &ge, dmls));
                }
                Ok(_) => {
                    notice = Some("反映対象の差分はありません。".to_string());
                }
                Err(err) => {
                    state.push_log("グリッド DML 生成".to_string(), err.clone(), true, None);
                    notice = Some(err);
                }
            }
        }

        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            if ui.button("閉じる").clicked() {
                keep_editor = false;
            }
            ui.menu_button("エクスポート", |ui| {
                let current_rows = collect_active_rows(&ge);
                if ui.button("CSV をファイル保存...").clicked() {
                    ui.close_menu();
                    let content = export_grid_csv(&ge.table.columns, &current_rows);
                    let default_name = format!("{}.csv", ge.table.table_name);
                    notice = save_export_dialog(&default_name, "CSV", &["csv"], &content);
                }
                if ui.button("TSV をファイル保存...").clicked() {
                    ui.close_menu();
                    let content = export_grid_tsv(&ge.table.columns, &current_rows, true);
                    let default_name = format!("{}.tsv", ge.table.table_name);
                    notice = save_export_dialog(&default_name, "TSV", &["tsv"], &content);
                }
                if ui.button("INSERT 文をファイル保存...").clicked() {
                    ui.close_menu();
                    let content = export_grid_insert_sql(
                        &ge.table.table_name,
                        &ge.table.columns,
                        &current_rows,
                        ge.table.dialect,
                    );
                    let default_name = format!("{}_insert.sql", ge.table.table_name);
                    notice = save_export_dialog(&default_name, "SQL", &["sql"], &content);
                }
                ui.separator();
                if ui.button("全行を TSV コピー").clicked() {
                    ui.ctx()
                        .copy_text(export_grid_tsv(&ge.table.columns, &current_rows, true));
                    notice = Some("グリッド内容を TSV 形式でコピーしました。".to_string());
                    ui.close_menu();
                }
                if ui.button("全行を INSERT 文コピー").clicked() {
                    ui.ctx().copy_text(export_grid_insert_sql(
                        &ge.table.table_name,
                        &ge.table.columns,
                        &current_rows,
                        ge.table.dialect,
                    ));
                    notice = Some("全行の INSERT 文をコピーしました。".to_string());
                    ui.close_menu();
                }
            });
        });
    });

    // 2. WHERE / ORDER BY フィルタバー
    ui.horizontal(|ui| {
        ui.label(RichText::new("WHERE").small().strong());
        let where_resp = ui.add(
            egui::TextEdit::singleline(&mut ge.where_input)
                .desired_width(220.0)
                .hint_text("例: ID > 10 AND STATUS = 'A'"),
        );

        ui.label(RichText::new("ORDER BY").small().strong());
        let order_resp = ui.add(
            egui::TextEdit::singleline(&mut ge.order_input)
                .desired_width(170.0)
                .hint_text("例: ID DESC"),
        );

        let reload_clicked = ui.add_enabled(!state.busy, egui::Button::new("再読込")).clicked();
        let enter_submitted = (where_resp.lost_focus() || order_resp.lost_focus())
            && ui.input(|i| i.key_pressed(egui::Key::Enter));

        if (reload_clicked || enter_submitted) && !state.busy {
            if ge.patch.has_changes() {
                notice = Some(
                    "未反映の変更があるため再読込できません。「反映」または「変更破棄」を実行してください。"
                        .to_string(),
                );
            } else {
                actions.push(AppAction::LoadTableForEdit {
                    owner: ge.table.owner.clone(),
                    table_name: ge.table.table_name.clone(),
                    where_clause: ge.where_input.clone(),
                    order_by: ge.order_input.clone(),
                });
            }
        }

        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            if ge.patch.has_changes() {
                ui.label(
                    RichText::new(format!(
                        "未反映: 更新{}行 / 挿入{}行 / 削除{}行",
                        ge.patch.modified_existing_row_count(),
                        ge.patch.inserted_row_count(),
                        ge.patch.deleted_row_count()
                    ))
                    .small()
                    .strong()
                    .color(Color32::from_rgb(191, 100, 0)),
                );
            }
        });
    });

    ui.separator();

    if ge.table.columns.is_empty() {
        ui.label("表示する列がありません。");
        if keep_editor {
            state.grid_editor = Some(ge);
        }
        if let Some(n) = notice {
            state.status_notice = Some(n);
        }
        return;
    }

    // 3. グリッド本体向けのキーボード操作（エディタ外フォーカスのときのみ）
    let mut copy_text: Option<String> = None;
    let display_rows = ge.patch.display_rows();

    if !ui.ctx().wants_keyboard_input() && state.confirm_modal.is_none() {
        let mut paste_payload: Option<String> = None;
        let mut copy_requested = false;
        let mut f2_pressed = false;
        let mut del_pressed = false;
        let mut undo_pressed = false;
        let mut redo_pressed = false;

        ui.input(|i| {
            for ev in &i.events {
                match ev {
                    egui::Event::Copy => copy_requested = true,
                    egui::Event::Paste(s) => paste_payload = Some(s.clone()),
                    _ => {}
                }
            }
            if i.key_pressed(egui::Key::F2) {
                f2_pressed = true;
            }
            if i.key_pressed(egui::Key::Delete) {
                del_pressed = true;
            }
            if i.modifiers.command && i.key_pressed(egui::Key::Z) {
                if i.modifiers.shift {
                    redo_pressed = true;
                } else {
                    undo_pressed = true;
                }
            }
            if i.modifiers.command && i.key_pressed(egui::Key::Y) {
                redo_pressed = true;
            }
        });

        if copy_requested && let Some((r0, r1, c0, c1)) = ge.selected_rect() {
            copy_text = Some(copy_editor_range(
                &ge,
                &display_rows,
                r0,
                r1,
                c0,
                c1,
                false,
                false,
            ));
        }

        if can_write_grid {
            if undo_pressed {
                ge.patch.undo(&ge.table);
            } else if redo_pressed {
                ge.patch.redo(&ge.table);
            } else if f2_pressed && let Some((disp_r, col_c)) = ge.selection_focus {
                start_inline_edit(&mut ge, &display_rows, disp_r, col_c);
            } else if del_pressed && let Some((r0, r1, c0, c1)) = ge.selected_rect() {
                apply_null_to_rect(&mut ge, &display_rows, r0, r1, c0, c1);
            } else if let Some(tsv) = paste_payload
                && let Some((start_r, start_c)) = ge.selection_focus
            {
                let applied_cells = paste_tsv_into_grid(&mut ge, &display_rows, start_r, start_c, &tsv);
                if applied_cells > 0 {
                    notice = Some(format!("{applied_cells} セルへ TSV を貼り付けました。"));
                }
            }
        }
    }

    // 貼付や行追加後に表示行リストを最新化
    let display_rows = ge.patch.display_rows();
    let disp_row_count = display_rows.len();
    let col_count = ge.table.columns.len();
    let row_height = 24.0;

    let (primary_pressed, primary_down, shift_held, pointer_pos) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.modifiers.shift,
            i.pointer.interact_pos(),
        )
    });
    if !primary_down {
        ge.is_selecting = false;
    }

    let mut table = TableBuilder::new(ui)
        .id_salt("grid_edit_table")
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(Align::Center))
        .column(Column::exact(54.0));

    for _ in 0..col_count {
        table = table.column(Column::initial(135.0).at_least(55.0).clip(true));
    }

    let sel_rect = ge.selected_rect();
    let mut cell_buf = String::with_capacity(64);
    let mut selection_event: Option<SelectionEvent> = None;
    let mut dbl_clicked_cell: Option<(usize, usize)> = None;
    let mut ctx_set_null: Option<(usize, usize, usize, usize)> = None;
    let mut ctx_copy_range: Option<String> = None;
    let mut ctx_paste: Option<(usize, usize)> = None;
    let mut ctx_toggle_del: Option<usize> = None;
    let mut ctx_dup_row: Option<usize> = None;
    let mut finish_edit: Option<(usize, usize, String, bool)> = None;

    table
        .header(24.0, |mut header| {
            header.col(|ui| {
                ui.add(egui::Label::new(RichText::new("#").strong()).selectable(false));
                let resp = ui
                    .interact(
                        ui.max_rect(),
                        ui.id().with("ed_header_all"),
                        Sense::click(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    selection_event = Some(SelectionEvent::SelectAll);
                }
            });
            for (col_idx, col) in ge.table.columns.iter().enumerate() {
                header.col(|ui| {
                    ui.add(
                        egui::Label::new(RichText::new(col.name.as_str()).strong())
                            .selectable(false),
                    );
                    let mut resp = ui
                        .interact(
                            ui.max_rect(),
                            ui.id().with(("ed_col_hdr", col_idx)),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if !col.data_type.is_empty() {
                        resp = resp.on_hover_text(col.data_type.as_str());
                    }
                    if resp.clicked() {
                        selection_event = Some(SelectionEvent::SelectCol(col_idx));
                    }
                });
            }
        })
        .body(|body| {
            body.rows(row_height, display_rows.len(), |mut row| {
                let disp_row_idx = row.index();
                let kind = display_rows[disp_row_idx];
                let (logical_row, is_deleted, is_inserted) = match kind {
                    DisplayRowKind::Existing {
                        logical_row,
                        marked_deleted,
                    } => (logical_row, marked_deleted, false),
                    DisplayRowKind::Inserted { logical_row } => (logical_row, false, true),
                };
                let row_editable = can_write_grid
                    && (is_inserted || (!is_deleted && ge.table.can_edit_existing_row(logical_row)));

                // 行番号＋状態マーカー列
                row.col(|ui| {
                    if is_deleted {
                        ui.painter().rect_filled(
                            ui.max_rect(),
                            0.0,
                            Color32::from_rgba_unmultiplied(220, 60, 60, 55),
                        );
                    } else if is_inserted {
                        ui.painter().rect_filled(
                            ui.max_rect(),
                            0.0,
                            Color32::from_rgba_unmultiplied(46, 170, 90, 55),
                        );
                    }
                    let prefix = if is_deleted {
                        "-"
                    } else if is_inserted {
                        "+"
                    } else {
                        ""
                    };
                    let fg = if is_deleted {
                        Color32::from_rgb(200, 40, 40)
                    } else if is_inserted {
                        Color32::from_rgb(20, 140, 65)
                    } else {
                        theme.editor_gutter_fg
                    };
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("{prefix}{}", disp_row_idx + 1))
                                    .small()
                                    .color(fg),
                            )
                            .selectable(false),
                        );
                    });
                    let resp = ui
                        .interact(
                            ui.max_rect(),
                            ui.id().with(("ed_row_hdr", disp_row_idx)),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if resp.clicked() {
                        selection_event = Some(SelectionEvent::SelectRow(disp_row_idx));
                    }
                });

                for col_idx in 0..col_count {
                    row.col(|ui| {
                        let is_modified = !is_inserted && ge.patch.is_cell_modified(logical_row, col_idx);
                        let is_selected = sel_rect.is_some_and(|(r0, r1, c0, c1)| {
                            disp_row_idx >= r0
                                && disp_row_idx <= r1
                                && col_idx >= c0
                                && col_idx <= c1
                        });

                        let rect = ui.max_rect();
                        if is_deleted {
                            ui.painter().rect_filled(
                                rect,
                                0.0,
                                Color32::from_rgba_unmultiplied(220, 60, 60, 55),
                            );
                        } else if is_inserted {
                            ui.painter().rect_filled(
                                rect,
                                0.0,
                                Color32::from_rgba_unmultiplied(46, 170, 90, 55),
                            );
                        } else if is_modified {
                            ui.painter().rect_filled(
                                rect,
                                0.0,
                                Color32::from_rgba_unmultiplied(245, 190, 40, 85),
                            );
                        }

                        if is_selected {
                            if !is_deleted && !is_inserted && !is_modified {
                                ui.painter().rect_filled(rect, 0.0, theme.grid_selected_bg);
                            }
                            ui.painter().rect_stroke(
                                rect.shrink(0.5),
                                0.0,
                                Stroke::new(1.5_f32, Color32::from_rgb(45, 125, 225)),
                                StrokeKind::Inside,
                            );
                        }

                        // インライン編集中セルなら TextEdit を表示する
                        if ge.editing_cell == Some((logical_row, col_idx)) {
                            let edit_resp = ui.add(
                                egui::TextEdit::singleline(&mut ge.edit_buffer)
                                    .desired_width(ui.available_width())
                                    .id_source(("grid_inline_edit", logical_row, col_idx)),
                            );
                            if ge.edit_just_started {
                                edit_resp.request_focus();
                                ge.edit_just_started = false;
                            }
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                finish_edit = Some((logical_row, col_idx, String::new(), false));
                            } else if edit_resp.lost_focus() {
                                finish_edit =
                                    Some((logical_row, col_idx, ge.edit_buffer.clone(), true));
                            }
                            return;
                        }

                        let val = ge.patch.cell_value(&ge.table, logical_row, col_idx);
                        val.write_display_into(&mut cell_buf);
                        let mut rich = if val.is_null() {
                            RichText::new(&cell_buf).italics().color(theme.null_fg)
                        } else {
                            RichText::new(&cell_buf)
                        };
                        if is_deleted {
                            rich = rich.strikethrough();
                        }

                        if val.is_numeric() {
                            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                ui.add(egui::Label::new(rich).selectable(false));
                            });
                        } else {
                            ui.add(egui::Label::new(rich).selectable(false));
                        }

                        let resp = ui.interact(
                            rect,
                            ui.id().with(("grid_edit_cell", disp_row_idx, col_idx)),
                            Sense::click_and_drag(),
                        );
                        if primary_pressed && resp.hovered() {
                            selection_event =
                                Some(SelectionEvent::StartCell(disp_row_idx, col_idx));
                        } else if ge.is_selecting
                            && primary_down
                            && pointer_pos
                                .is_some_and(|p| rect.contains(p) && ui.clip_rect().contains(p))
                        {
                            selection_event =
                                Some(SelectionEvent::DragHover(disp_row_idx, col_idx));
                        }
                        if resp.secondary_clicked() {
                            selection_event =
                                Some(SelectionEvent::RightClick(disp_row_idx, col_idx));
                        }
                        if resp.double_clicked() && row_editable {
                            dbl_clicked_cell = Some((disp_row_idx, col_idx));
                        }
                        resp.context_menu(|ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                            if ui
                                .add_enabled(row_editable, egui::Button::new("セルを編集 (F2)"))
                                .clicked()
                            {
                                dbl_clicked_cell = Some((disp_row_idx, col_idx));
                                ui.close_menu();
                            }
                            if ui
                                .add_enabled(row_editable, egui::Button::new("NULL を設定 (Del)"))
                                .clicked()
                            {
                                if let Some((r0, r1, c0, c1)) = sel_rect
                                    && disp_row_idx >= r0
                                    && disp_row_idx <= r1
                                    && col_idx >= c0
                                    && col_idx <= c1
                                {
                                    ctx_set_null = Some((r0, r1, c0, c1));
                                } else {
                                    ctx_set_null =
                                        Some((disp_row_idx, disp_row_idx, col_idx, col_idx));
                                }
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("選択範囲を TSV コピー").clicked() {
                                let (r0, r1, c0, c1) = sel_rect.unwrap_or((
                                    disp_row_idx,
                                    disp_row_idx,
                                    col_idx,
                                    col_idx,
                                ));
                                ctx_copy_range = Some(copy_editor_range(
                                    &ge,
                                    &display_rows,
                                    r0,
                                    r1,
                                    c0,
                                    c1,
                                    false,
                                    false,
                                ));
                                ui.close_menu();
                            }
                            if ui.button("選択範囲を TSV コピー (カラム名付き)").clicked() {
                                let (r0, r1, c0, c1) = sel_rect.unwrap_or((
                                    disp_row_idx,
                                    disp_row_idx,
                                    col_idx,
                                    col_idx,
                                ));
                                ctx_copy_range = Some(copy_editor_range(
                                    &ge,
                                    &display_rows,
                                    r0,
                                    r1,
                                    c0,
                                    c1,
                                    false,
                                    true,
                                ));
                                ui.close_menu();
                            }
                            if ui.button("選択範囲を CSV コピー").clicked() {
                                let (r0, r1, c0, c1) = sel_rect.unwrap_or((
                                    disp_row_idx,
                                    disp_row_idx,
                                    col_idx,
                                    col_idx,
                                ));
                                ctx_copy_range = Some(copy_editor_range(
                                    &ge,
                                    &display_rows,
                                    r0,
                                    r1,
                                    c0,
                                    c1,
                                    true,
                                    false,
                                ));
                                ui.close_menu();
                            }
                            if ui.button("選択範囲を CSV コピー (カラム名付き)").clicked() {
                                let (r0, r1, c0, c1) = sel_rect.unwrap_or((
                                    disp_row_idx,
                                    disp_row_idx,
                                    col_idx,
                                    col_idx,
                                ));
                                ctx_copy_range = Some(copy_editor_range(
                                    &ge,
                                    &display_rows,
                                    r0,
                                    r1,
                                    c0,
                                    c1,
                                    true,
                                    true,
                                ));
                                ui.close_menu();
                            }
                            if ui
                                .add_enabled(can_write_grid, egui::Button::new("TSV 貼付ガイド (Ctrl+V)"))
                                .clicked()
                            {
                                ctx_paste = Some((disp_row_idx, col_idx));
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui
                                .add_enabled(can_write_grid, egui::Button::new("この行を複製"))
                                .clicked()
                            {
                                ctx_dup_row = Some(logical_row);
                                ui.close_menu();
                            }
                            let row_del_label = if is_deleted {
                                "この行の削除を取り消す"
                            } else {
                                "この行を削除"
                            };
                            if ui
                                .add_enabled(can_write_grid, egui::Button::new(row_del_label))
                                .clicked()
                            {
                                ctx_toggle_del = Some(logical_row);
                                ui.close_menu();
                            }
                        });
                    });
                }
            });
        });

    if let Some((lr, c, buf, commit)) = finish_edit {
        ge.editing_cell = None;
        if commit && let Some(col_meta) = ge.table.columns.get(c) {
            let parsed = parse_cell_input(&buf, col_meta, false);
            ge.patch.set_cell(&ge.table, lr, c, parsed);
        }
    }
    if let Some(ev) = selection_event {
        apply_selection_event(
            &mut ge.selection_anchor,
            &mut ge.selection_focus,
            &mut ge.is_selecting,
            disp_row_count,
            col_count,
            shift_held,
            ev,
        );
    }
    if let Some((disp_r, col_c)) = dbl_clicked_cell {
        ge.selection_anchor = Some((disp_r, col_c));
        ge.selection_focus = Some((disp_r, col_c));
        start_inline_edit(&mut ge, &display_rows, disp_r, col_c);
    }
    if let Some((r0, r1, c0, c1)) = ctx_set_null {
        apply_null_to_rect(&mut ge, &display_rows, r0, r1, c0, c1);
    }
    if let Some(lr) = ctx_dup_row {
        ge.patch.duplicate_row(&ge.table, lr);
    }
    if let Some(lr) = ctx_toggle_del {
        ge.patch.toggle_delete_row(&ge.table, lr);
    }
    if let Some((_r, _c)) = ctx_paste {
        notice = Some("セルを選択した状態で Ctrl+V を押すと Excel 形式の TSV を貼り付けできます。".to_string());
    }
    if let Some(txt) = ctx_copy_range.or(copy_text) {
        ui.ctx().copy_text(txt);
    }
    if let Some(modal) = pending_confirm {
        state.confirm_modal = Some(modal);
    }
    if let Some(n) = notice {
        state.status_notice = Some(n);
    }
    if keep_editor {
        state.grid_editor = Some(ge);
    }
}

/// 指定した表示セルのインライン編集を開始する。
fn start_inline_edit(
    ge: &mut GridEditorState,
    display_rows: &[DisplayRowKind],
    disp_row: usize,
    col: usize,
) {
    let Some(kind) = display_rows.get(disp_row) else {
        return;
    };
    let (logical_row, editable) = match *kind {
        DisplayRowKind::Existing {
            logical_row,
            marked_deleted,
        } => (
            logical_row,
            !marked_deleted && ge.table.can_edit_existing_row(logical_row),
        ),
        DisplayRowKind::Inserted { logical_row } => (logical_row, true),
    };
    if !editable || col >= ge.table.columns.len() {
        return;
    }
    let val = ge.patch.cell_value(&ge.table, logical_row, col);
    ge.edit_buffer = if val.is_null() {
        String::new()
    } else {
        val.copy_str().into_owned()
    };
    ge.editing_cell = Some((logical_row, col));
    ge.edit_just_started = true;
}

/// 指定した表示矩形範囲内の全編集可能セルへ `NULL` を設定する（1 ステップで Undo 可能）。
fn apply_null_to_rect(
    ge: &mut GridEditorState,
    display_rows: &[DisplayRowKind],
    r0: usize,
    r1: usize,
    c0: usize,
    c1: usize,
) {
    let mut ops = Vec::new();
    for disp_r in r0..=r1 {
        let Some(kind) = display_rows.get(disp_r) else {
            continue;
        };
        let (logical_row, editable) = match *kind {
            DisplayRowKind::Existing {
                logical_row,
                marked_deleted,
            } => (
                logical_row,
                !marked_deleted && ge.table.can_edit_existing_row(logical_row),
            ),
            DisplayRowKind::Inserted { logical_row } => (logical_row, true),
        };
        if !editable {
            continue;
        }
        for col in c0..=c1 {
            if col >= ge.table.columns.len() {
                continue;
            }
            let old_val = ge.patch.cell_value(&ge.table, logical_row, col).clone();
            if !old_val.is_null() {
                ops.push(GridEditOp::UpdateCell {
                    row: logical_row,
                    col,
                    old_val,
                    new_val: CellValue::Null,
                });
            }
        }
    }
    ge.patch.apply_batch(&ge.table, ops);
}

/// クリップボードの Excel TSV テキストを開始セル位置から矩形範囲へ貼り付ける。
fn paste_tsv_into_grid(
    ge: &mut GridEditorState,
    display_rows: &[DisplayRowKind],
    start_disp_row: usize,
    start_col: usize,
    tsv_text: &str,
) -> usize {
    let parsed_rows = parse_tsv(tsv_text);
    if parsed_rows.is_empty() {
        return 0;
    }
    let mut ops = Vec::new();
    let mut pasted_count = 0usize;

    for (r_off, cells) in parsed_rows.iter().enumerate() {
        let target_disp = start_disp_row + r_off;
        let target_logical = if target_disp < display_rows.len() {
            match display_rows[target_disp] {
                DisplayRowKind::Existing {
                    logical_row,
                    marked_deleted,
                } => {
                    if marked_deleted || !ge.table.can_edit_existing_row(logical_row) {
                        continue;
                    }
                    logical_row
                }
                DisplayRowKind::Inserted { logical_row } => logical_row,
            }
        } else {
            // 既存表示行を超える行は新規行として追加してからセル値を埋める
            let mut new_vals = vec![CellValue::Null; ge.table.columns.len()];
            for (c_off, raw) in cells.iter().enumerate() {
                let col = start_col + c_off;
                if let Some(col_meta) = ge.table.columns.get(col) {
                    new_vals[col] = parse_cell_input(raw, col_meta, true);
                    pasted_count += 1;
                }
            }
            let _new_lr = ge.patch.insert_row(&ge.table, Some(new_vals));
            continue;
        };

        for (c_off, raw) in cells.iter().enumerate() {
            let col = start_col + c_off;
            let Some(col_meta) = ge.table.columns.get(col) else {
                continue;
            };
            let new_val = parse_cell_input(raw, col_meta, true);
            let old_val = ge.patch.cell_value(&ge.table, target_logical, col).clone();
            if old_val != new_val {
                ops.push(GridEditOp::UpdateCell {
                    row: target_logical,
                    col,
                    old_val,
                    new_val,
                });
            }
            pasted_count += 1;
        }
    }

    ge.patch.apply_batch(&ge.table, ops);
    pasted_count
}

/// 反映（Apply）実行前の確認モーダルを構築する。
fn build_apply_confirm_modal(
    is_prod: bool,
    ge: &GridEditorState,
    dmls: Vec<ParameterizedDml>,
) -> ConfirmModal {
    let preview = format_dml_preview_script(&dmls);
    ConfirmModal {
        title: if is_prod {
            "本番環境 テーブル変更反映の確認".to_string()
        } else {
            "テーブル変更反映の確認".to_string()
        },
        message: format!(
            "テーブル {} に対して以下の変更（更新 {} 行 / 挿入 {} 行 / 削除 {} 行、計 {} 文）を適用します。\n※ SAVEPOINT 保護下で実行され、自動コミットは行いません。",
            ge.table.table_name,
            ge.patch.modified_existing_row_count(),
            ge.patch.inserted_row_count(),
            ge.patch.deleted_row_count(),
            dmls.len()
        ),
        sql_preview: Some(preview),
        is_danger: is_prod || ge.patch.deleted_row_count() > 0,
        confirm_label: "DB へ反映 (未コミット)".to_string(),
        action: ConfirmAction::ApplyGridDml { dmls },
    }
}

/// アクティブプロファイルから SQL 方言を推定する。
fn active_dialect(state: &UiState) -> SqlDialect {
    match state
        .active_profile
        .as_ref()
        .map(|p| p.db_type)
        .unwrap_or(ququ_core::DbType::Sqlite)
    {
        ququ_core::DbType::Oracle => SqlDialect::Oracle,
        ququ_core::DbType::Sqlite => SqlDialect::Sqlite,
    }
}

/// 選択中のテーブル編集行をプレビュー・エクスポート向けに実体化する（削除マーク行は除外）。
fn collect_active_rows(ge: &GridEditorState) -> Vec<Vec<CellValue>> {
    let col_count = ge.table.columns.len();
    ge.patch
        .display_rows()
        .into_iter()
        .filter_map(|k| match k {
            DisplayRowKind::Existing {
                logical_row,
                marked_deleted,
            } => (!marked_deleted).then_some(logical_row),
            DisplayRowKind::Inserted { logical_row } => Some(logical_row),
        })
        .map(|lr| {
            (0..col_count)
                .map(|c| ge.patch.cell_value(&ge.table, lr, c).clone())
                .collect()
        })
        .collect()
}

fn export_grid_csv(columns: &[ColumnMeta], rows: &[Vec<CellValue>]) -> String {
    export_csv(
        columns,
        rows.iter().map(|r| r.iter().collect::<Vec<_>>()),
    )
}

fn export_grid_tsv(columns: &[ColumnMeta], rows: &[Vec<CellValue>], with_header: bool) -> String {
    export_tsv(
        columns,
        rows.iter().map(|r| r.iter().collect::<Vec<_>>()),
        with_header,
    )
}

fn export_grid_insert_sql(
    table_name: &str,
    columns: &[ColumnMeta],
    rows: &[Vec<CellValue>],
    dialect: SqlDialect,
) -> String {
    export_insert_sql(
        table_name,
        columns,
        rows.iter().map(|r| r.iter().collect::<Vec<_>>()),
        dialect,
    )
}

fn export_result_csv(rs: &ResultSet) -> String {
    let cols = rs.col_count();
    let row_refs: Vec<Vec<&CellValue>> = (0..rs.row_count)
        .map(|r| {
            (0..cols)
                .map(|c| rs.get(r, c).unwrap_or(&CellValue::Null))
                .collect()
        })
        .collect();
    export_csv(&rs.columns, row_refs)
}

fn export_result_insert_sql(rs: &ResultSet, table_name: &str, dialect: SqlDialect) -> String {
    let cols = rs.col_count();
    let row_refs: Vec<Vec<&CellValue>> = (0..rs.row_count)
        .map(|r| {
            (0..cols)
                .map(|c| rs.get(r, c).unwrap_or(&CellValue::Null))
                .collect()
        })
        .collect();
    export_insert_sql(table_name, &rs.columns, row_refs, dialect)
}

/// 選択範囲の行・列スライスを指定フォーマット（TSV / CSV、カラム名有無）で文字列化する。
fn format_range_text(
    all_columns: &[ColumnMeta],
    sub_rows: Vec<Vec<&CellValue>>,
    c0: usize,
    c1: usize,
    is_csv: bool,
    with_header: bool,
) -> String {
    let cols_slice: Vec<ColumnMeta> = if with_header {
        (c0..=c1)
            .filter_map(|c| all_columns.get(c).cloned())
            .collect()
    } else {
        Vec::new()
    };
    if is_csv {
        export_csv_with_header(&cols_slice, sub_rows, with_header)
    } else {
        export_tsv(&cols_slice, sub_rows, with_header)
    }
}

/// 編集グリッドの指定矩形範囲を Excel 互換 TSV / CSV 文字列としてコピー用に出力する。
#[allow(clippy::too_many_arguments)]
fn copy_editor_range(
    ge: &GridEditorState,
    display_rows: &[DisplayRowKind],
    r0: usize,
    r1: usize,
    c0: usize,
    c1: usize,
    is_csv: bool,
    with_header: bool,
) -> String {
    let mut sub_rows = Vec::new();
    for disp_r in r0..=r1 {
        let Some(kind) = display_rows.get(disp_r) else {
            continue;
        };
        let lr = match *kind {
            DisplayRowKind::Existing { logical_row, .. }
            | DisplayRowKind::Inserted { logical_row } => logical_row,
        };
        let row_vec: Vec<&CellValue> = (c0..=c1)
            .filter(|&c| c < ge.table.columns.len())
            .map(|c| ge.patch.cell_value(&ge.table, lr, c))
            .collect();
        sub_rows.push(row_vec);
    }
    format_range_text(&ge.table.columns, sub_rows, c0, c1, is_csv, with_header)
}

/// 読み取り専用グリッドの指定矩形範囲を TSV / CSV 文字列としてコピー用に出力する。
fn copy_readonly_range(
    rs: &ResultSet,
    r0: usize,
    r1: usize,
    c0: usize,
    c1: usize,
    is_csv: bool,
    with_header: bool,
) -> String {
    let mut sub_rows = Vec::new();
    for r in r0..=r1 {
        if r >= rs.row_count {
            break;
        }
        let row_vec: Vec<&CellValue> = (c0..=c1)
            .filter(|&c| c < rs.col_count())
            .map(|c| rs.get(r, c).unwrap_or(&CellValue::Null))
            .collect();
        sub_rows.push(row_vec);
    }
    format_range_text(&rs.columns, sub_rows, c0, c1, is_csv, with_header)
}

/// ファイル保存ダイアログを開いてエクスポート結果を書き込む。
fn save_export_dialog(
    default_name: &str,
    filter_name: &str,
    extensions: &[&str],
    content: &str,
) -> Option<String> {
    let path = rfd::FileDialog::new()
        .add_filter(filter_name, extensions)
        .set_file_name(default_name)
        .save_file()?;

    match fs::write(&path, content) {
        Ok(()) => Some(format!("ファイルへエクスポートしました: {}", path.display())),
        Err(e) => Some(format!("ファイル保存に失敗しました: {e}")),
    }
}

/// 結果セットを Excel 互換の TSV（タブ区切り＋必要時ダブルクォートエスケープ）文字列へ変換する。
fn result_set_to_tsv(rs: &ResultSet, with_header: bool) -> String {
    let cols = rs.col_count();
    let row_refs: Vec<Vec<&CellValue>> = (0..rs.row_count)
        .map(|r| {
            (0..cols)
                .map(|c| rs.get(r, c).unwrap_or(&CellValue::Null))
                .collect()
        })
        .collect();
    export_tsv(&rs.columns, row_refs, with_header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ququ_core::{CellValue, ColumnMeta};

    #[test]
    fn exports_tsv_with_excel_escaping_and_null_handling() {
        let mut rs = ResultSet::with_columns(vec![
            ColumnMeta {
                name: "ID".into(),
                data_type: "INT".into(),
                nullable: false,
            },
            ColumnMeta {
                name: "NOTE".into(),
                data_type: "TEXT".into(),
                nullable: true,
            },
        ]);
        rs.push_row(vec![
            CellValue::Int(1),
            CellValue::Text("a\"b\nc".into()),
        ]);
        rs.push_row(vec![CellValue::Int(2), CellValue::Null]);

        let tsv = result_set_to_tsv(&rs, true);
        assert_eq!(tsv, "ID\tNOTE\n1\t\"a\"\"b\nc\"\n2\t\n");
    }

    #[test]
    fn selection_events_support_drag_headers_and_right_click_preservation() {
        let mut anchor = None;
        let mut focus = None;
        let mut selecting = false;

        // ドラッグ開始と範囲拡張
        apply_selection_event(
            &mut anchor,
            &mut focus,
            &mut selecting,
            5,
            4,
            false,
            SelectionEvent::StartCell(1, 1),
        );
        assert!(selecting);
        assert_eq!(rect_from_points(anchor, focus), Some((1, 1, 1, 1)));

        apply_selection_event(
            &mut anchor,
            &mut focus,
            &mut selecting,
            5,
            4,
            false,
            SelectionEvent::DragHover(3, 2),
        );
        assert_eq!(rect_from_points(anchor, focus), Some((1, 3, 1, 2)));

        // 選択範囲内の右クリックでは選択範囲が維持される
        apply_selection_event(
            &mut anchor,
            &mut focus,
            &mut selecting,
            5,
            4,
            false,
            SelectionEvent::RightClick(2, 2),
        );
        assert_eq!(rect_from_points(anchor, focus), Some((1, 3, 1, 2)));

        // 選択範囲外の右クリックはそのセルへ切り替わる
        apply_selection_event(
            &mut anchor,
            &mut focus,
            &mut selecting,
            5,
            4,
            false,
            SelectionEvent::RightClick(0, 0),
        );
        assert_eq!(rect_from_points(anchor, focus), Some((0, 0, 0, 0)));

        // 行ヘッダ・列ヘッダ・全選択
        apply_selection_event(
            &mut anchor,
            &mut focus,
            &mut selecting,
            5,
            4,
            false,
            SelectionEvent::SelectRow(2),
        );
        assert_eq!(rect_from_points(anchor, focus), Some((2, 2, 0, 3)));

        apply_selection_event(
            &mut anchor,
            &mut focus,
            &mut selecting,
            5,
            4,
            false,
            SelectionEvent::SelectCol(1),
        );
        assert_eq!(rect_from_points(anchor, focus), Some((0, 4, 1, 1)));

        apply_selection_event(
            &mut anchor,
            &mut focus,
            &mut selecting,
            5,
            4,
            false,
            SelectionEvent::SelectAll,
        );
        assert_eq!(rect_from_points(anchor, focus), Some((0, 4, 0, 3)));
    }

    #[test]
    fn copies_readonly_range_as_tsv_and_csv_with_optional_header() {
        let mut rs = ResultSet::with_columns(vec![
            ColumnMeta {
                name: "ID".into(),
                data_type: "INT".into(),
                nullable: false,
            },
            ColumnMeta {
                name: "NAME".into(),
                data_type: "TEXT".into(),
                nullable: true,
            },
        ]);
        rs.push_row(vec![CellValue::Int(1), CellValue::Text("A, B".into())]);
        rs.push_row(vec![CellValue::Int(2), CellValue::Null]);

        assert_eq!(copy_readonly_range(&rs, 0, 1, 0, 1, false, false), "1\tA, B\n2\t\n");
        assert_eq!(
            copy_readonly_range(&rs, 0, 1, 0, 1, false, true),
            "ID\tNAME\n1\tA, B\n2\t\n"
        );
        assert_eq!(
            copy_readonly_range(&rs, 0, 1, 0, 1, true, false),
            "1,\"A, B\"\n2,\n"
        );
        assert_eq!(
            copy_readonly_range(&rs, 0, 1, 0, 1, true, true),
            "ID,NAME\n1,\"A, B\"\n2,\n"
        );
    }
}
