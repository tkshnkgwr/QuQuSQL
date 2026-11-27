//! 右側のオブジェクトブラウザ（オブジェクト一覧ペイン・カラム詳細ペイン）の UI 実装モジュール。

use crate::state::{AppAction, UiState};
use crate::theme::AppTheme;
use egui::{Align, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use ququ_core::{ColumnDetail, ObjectType};

/// 右上ペイン：スキーマ選択・オブジェクト種別・フィルタ・オブジェクト一覧を描画する。
pub fn render_object_list_pane(
    ui: &mut Ui,
    state: &mut UiState,
    theme: &AppTheme,
    actions: &mut Vec<AppAction>,
) {
    let is_connected = state.active_profile.is_some();

    ui.vertical(|ui| {
        // owner / type / filter の操作ヘッダー部
        egui::Grid::new("obj_browser_controls")
            .num_columns(2)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                ui.label("owner");
                let prev_owner = state.selected_owner.clone();
                egui::ComboBox::from_id_salt("owner_combo")
                    .selected_text(if state.selected_owner.is_empty() {
                        "(未選択)"
                    } else {
                        &state.selected_owner
                    })
                    .width(ui.available_width().max(100.0))
                    .show_ui(ui, |ui| {
                        for owner in &state.owners {
                            ui.selectable_value(&mut state.selected_owner, owner.clone(), owner);
                        }
                    });
                if is_connected
                    && state.selected_owner != prev_owner
                    && !state.selected_owner.is_empty()
                {
                    actions.push(AppAction::LoadObjects {
                        owner: state.selected_owner.clone(),
                        object_type: state.selected_object_type.clone(),
                    });
                }
                ui.end_row();

                ui.label("type");
                let prev_type = state.selected_object_type.clone();
                egui::ComboBox::from_id_salt("type_combo")
                    .selected_text(state.selected_object_type.as_str())
                    .width(ui.available_width().max(100.0))
                    .show_ui(ui, |ui| {
                        for ty in ObjectType::standard_types() {
                            ui.selectable_value(
                                &mut state.selected_object_type,
                                ty.clone(),
                                ty.as_str(),
                            );
                        }
                    });
                if is_connected
                    && state.selected_object_type != prev_type
                    && !state.selected_owner.is_empty()
                {
                    actions.push(AppAction::LoadObjects {
                        owner: state.selected_owner.clone(),
                        object_type: state.selected_object_type.clone(),
                    });
                }
                ui.end_row();

                ui.label("filter");
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 48.0).max(60.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut state.object_filter)
                            .desired_width(w)
                            .hint_text("絞り込み..."),
                    );
                    if ui.button("clear").clicked() {
                        state.object_filter.clear();
                    }
                });
                ui.end_row();
            });

        ui.separator();

        // フィルタ文字列に一致するオブジェクトのインデックスを抽出
        let filter_lower = state.object_filter.trim().to_ascii_lowercase();
        let filtered_indices: Vec<usize> = state
            .objects
            .iter()
            .enumerate()
            .filter(|(_, obj)| {
                filter_lower.is_empty()
                    || obj.name.to_ascii_lowercase().contains(&filter_lower)
                    || obj.comments.to_ascii_lowercase().contains(&filter_lower)
            })
            .map(|(i, _)| i)
            .collect();

        let mut new_selection: Option<usize> = None;
        let mut insert_text: Option<String> = None;
        let mut copy_text: Option<String> = None;
        let mut load_ddl_target: Option<(String, ObjectType, String)> = None;
        let mut full_select_target: Option<(usize, String, String)> = None;
        let mut edit_grid_target: Option<(String, String)> = None;

        TableBuilder::new(ui)
            .id_salt("object_list_table")
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(Align::Center))
            .column(Column::initial(160.0).at_least(80.0).clip(true))
            .column(Column::remainder().at_least(60.0).clip(true))
            .header(22.0, |mut header| {
                header.col(|ui| {
                    ui.add(
                        egui::Label::new(RichText::new("OBJECT_NAME").strong()).selectable(false),
                    );
                });
                header.col(|ui| {
                    ui.add(egui::Label::new(RichText::new("COMMENTS").strong()).selectable(false));
                });
            })
            .body(|body| {
                body.rows(20.0, filtered_indices.len(), |mut row| {
                    let orig_idx = filtered_indices[row.index()];
                    let obj = &state.objects[orig_idx];
                    let is_selected = state.selected_object_idx == Some(orig_idx);

                    row.col(|ui| {
                        if is_selected {
                            ui.painter().rect_filled(ui.max_rect(), 0.0, theme.grid_selected_bg);
                        }
                        ui.add(egui::Label::new(RichText::new(&obj.name)).selectable(false));
                        // 文字の後ろの余白を含め、セル矩形全体でクリック・右クリック・指アイコンを受け付ける
                        let resp = ui
                            .interact(
                                ui.max_rect(),
                                ui.id().with(("obj_cell", orig_idx, 0)),
                                Sense::click(),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if resp.clicked() {
                            new_selection = Some(orig_idx);
                        }
                        if resp.double_clicked() {
                            insert_text = Some(obj.name.clone());
                        }
                        resp.context_menu(|ui| {
                            if matches!(obj.object_type, ObjectType::Table | ObjectType::View)
                                && ui.button("グリッドで開く").clicked()
                            {
                                edit_grid_target = Some((obj.owner.clone(), obj.name.clone()));
                                ui.close_menu();
                            }
                            if ui.button("エディタへ名前挿入").clicked() {
                                insert_text = Some(obj.name.clone());
                                ui.close_menu();
                            }
                            if ui.button("SELECT 文を挿入").clicked() {
                                insert_text = Some(format!("SELECT * FROM {};\n", obj.name));
                                ui.close_menu();
                            }
                            if ui.button("SELECT 文を挿入 (全項目)").clicked() {
                                full_select_target =
                                    Some((orig_idx, obj.owner.clone(), obj.name.clone()));
                                ui.close_menu();
                            }
                            if ui.button("DDL を取得").clicked() {
                                load_ddl_target = Some((
                                    obj.owner.clone(),
                                    obj.object_type.clone(),
                                    obj.name.clone(),
                                ));
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("オブジェクト名をコピー").clicked() {
                                copy_text = Some(obj.name.clone());
                                ui.close_menu();
                            }
                        });
                    });

                    row.col(|ui| {
                        if is_selected {
                            ui.painter().rect_filled(ui.max_rect(), 0.0, theme.grid_selected_bg);
                        }
                        ui.add(egui::Label::new(&obj.comments).selectable(false));
                        let resp = ui
                            .interact(
                                ui.max_rect(),
                                ui.id().with(("obj_cell", orig_idx, 1)),
                                Sense::click(),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if resp.clicked() {
                            new_selection = Some(orig_idx);
                        }
                        if resp.double_clicked() {
                            insert_text = Some(obj.name.clone());
                        }
                    });
                });
            });

        // 選択行が変わったらカラム詳細を非同期取得する
        if let Some(idx) = new_selection
            && state.selected_object_idx != Some(idx)
        {
            state.selected_object_idx = Some(idx);
            if let Some(obj) = state.objects.get(idx) {
                actions.push(AppAction::LoadColumns {
                    owner: obj.owner.clone(),
                    table_name: obj.name.clone(),
                });
            }
        }

        // 右クリック「SELECT 文を挿入 (全項目)」の処理
        if let Some((idx, owner, table_name)) = full_select_target {
            if state.selected_object_idx == Some(idx) && !state.columns.is_empty() {
                let sql = format_full_select_sql(&table_name, &state.columns);
                state.insert_into_active_editor(&sql);
            } else {
                // カラム未取得のオブジェクトの場合は先にカラム取得を要求して到着後に挿入する
                state.selected_object_idx = Some(idx);
                actions.push(AppAction::InsertFullSelect { owner, table_name });
            }
        }

        // 右クリック「DDL を取得」の処理
        if let Some((owner, object_type, name)) = load_ddl_target {
            actions.push(AppAction::LoadDdl {
                owner,
                object_type,
                name,
            });
        }

        // 右クリック「グリッドで開く (OSqlGrid)」の処理
        if let Some((owner, table_name)) = edit_grid_target {
            state.bottom_tab = crate::state::BottomTab::Grid;
            actions.push(AppAction::LoadTableForEdit {
                owner,
                table_name,
                where_clause: String::new(),
                order_by: String::new(),
            });
        }

        if let Some(snippet) = insert_text {
            state.insert_into_active_editor(&snippet);
        }
        if let Some(txt) = copy_text {
            ui.ctx().copy_text(txt);
        }
    });
}

/// 右下ペイン：選択中オブジェクトのカラム詳細一覧とフィルタを描画する。
pub fn render_object_detail_pane(ui: &mut Ui, state: &mut UiState, theme: &AppTheme) {
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut true, true, "Column");
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{} 列", state.columns.len()))
                        .small()
                        .color(egui::Color32::GRAY),
                );
            });
        });

        ui.horizontal(|ui| {
            ui.label("filter");
            let w = (ui.available_width() - 48.0).max(60.0);
            ui.add(
                egui::TextEdit::singleline(&mut state.column_filter)
                    .desired_width(w)
                    .hint_text("カラム絞り込み..."),
            );
            if ui.button("clear").clicked() {
                state.column_filter.clear();
            }
        });

        ui.separator();

        let filter_lower = state.column_filter.trim().to_ascii_lowercase();
        let filtered: Vec<usize> = state
            .columns
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                filter_lower.is_empty()
                    || c.name.to_ascii_lowercase().contains(&filter_lower)
                    || c.comments.to_ascii_lowercase().contains(&filter_lower)
            })
            .map(|(i, _)| i)
            .collect();

        let mut new_col_selection: Option<usize> = None;
        let mut insert_col: Option<String> = None;
        let mut copy_col_name: Option<String> = None;
        let mut copy_all_cols = false;

        TableBuilder::new(ui)
            .id_salt("column_detail_table")
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(Align::Center))
            .column(Column::initial(120.0).at_least(60.0).clip(true))
            .column(Column::initial(85.0).at_least(50.0).clip(true))
            .column(Column::exact(46.0))
            .column(Column::exact(28.0))
            .column(Column::remainder().at_least(50.0).clip(true))
            .header(22.0, |mut header| {
                header.col(|ui| {
                    ui.add(
                        egui::Label::new(RichText::new("column name").strong()).selectable(false),
                    );
                });
                header.col(|ui| {
                    ui.add(
                        egui::Label::new(RichText::new("data type").strong()).selectable(false),
                    );
                });
                header.col(|ui| {
                    ui.add(egui::Label::new(RichText::new("len").strong()).selectable(false));
                });
                header.col(|ui| {
                    ui.add(egui::Label::new(RichText::new("n").strong()).selectable(false));
                });
                header.col(|ui| {
                    ui.add(egui::Label::new(RichText::new("comments").strong()).selectable(false));
                });
            })
            .body(|body| {
                body.rows(20.0, filtered.len(), |mut row| {
                    let orig_idx = filtered[row.index()];
                    let col = &state.columns[orig_idx];
                    let is_selected = state.selected_column_idx == Some(orig_idx);

                    row.col(|ui| {
                        if is_selected {
                            ui.painter().rect_filled(ui.max_rect(), 0.0, theme.grid_selected_bg);
                        }
                        let label = if col.is_pk {
                            RichText::new(format!("* {}", col.name)).strong()
                        } else {
                            RichText::new(&col.name)
                        };
                        ui.add(egui::Label::new(label).selectable(false));
                        let resp = ui.interact(
                            ui.max_rect(),
                            ui.id().with(("col_cell", orig_idx, 0)),
                            Sense::click(),
                        );
                        handle_column_cell_interaction(
                            resp,
                            orig_idx,
                            &col.name,
                            &mut new_col_selection,
                            &mut insert_col,
                            &mut copy_col_name,
                            &mut copy_all_cols,
                        );
                    });
                    row.col(|ui| {
                        if is_selected {
                            ui.painter().rect_filled(ui.max_rect(), 0.0, theme.grid_selected_bg);
                        }
                        ui.add(egui::Label::new(&col.data_type).selectable(false));
                        let resp = ui.interact(
                            ui.max_rect(),
                            ui.id().with(("col_cell", orig_idx, 1)),
                            Sense::click(),
                        );
                        handle_column_cell_interaction(
                            resp,
                            orig_idx,
                            &col.name,
                            &mut new_col_selection,
                            &mut insert_col,
                            &mut copy_col_name,
                            &mut copy_all_cols,
                        );
                    });
                    row.col(|ui| {
                        if is_selected {
                            ui.painter().rect_filled(ui.max_rect(), 0.0, theme.grid_selected_bg);
                        }
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            ui.add(egui::Label::new(&col.length).selectable(false));
                        });
                        let resp = ui.interact(
                            ui.max_rect(),
                            ui.id().with(("col_cell", orig_idx, 2)),
                            Sense::click(),
                        );
                        handle_column_cell_interaction(
                            resp,
                            orig_idx,
                            &col.name,
                            &mut new_col_selection,
                            &mut insert_col,
                            &mut copy_col_name,
                            &mut copy_all_cols,
                        );
                    });
                    row.col(|ui| {
                        if is_selected {
                            ui.painter().rect_filled(ui.max_rect(), 0.0, theme.grid_selected_bg);
                        }
                        ui.add(
                            egui::Label::new(if col.nullable { "Y" } else { "N" })
                                .selectable(false),
                        );
                        let resp = ui.interact(
                            ui.max_rect(),
                            ui.id().with(("col_cell", orig_idx, 3)),
                            Sense::click(),
                        );
                        handle_column_cell_interaction(
                            resp,
                            orig_idx,
                            &col.name,
                            &mut new_col_selection,
                            &mut insert_col,
                            &mut copy_col_name,
                            &mut copy_all_cols,
                        );
                    });
                    row.col(|ui| {
                        if is_selected {
                            ui.painter().rect_filled(ui.max_rect(), 0.0, theme.grid_selected_bg);
                        }
                        ui.add(egui::Label::new(&col.comments).selectable(false));
                        let resp = ui.interact(
                            ui.max_rect(),
                            ui.id().with(("col_cell", orig_idx, 4)),
                            Sense::click(),
                        );
                        handle_column_cell_interaction(
                            resp,
                            orig_idx,
                            &col.name,
                            &mut new_col_selection,
                            &mut insert_col,
                            &mut copy_col_name,
                            &mut copy_all_cols,
                        );
                    });
                });
            });

        if let Some(idx) = new_col_selection {
            state.selected_column_idx = Some(idx);
        }
        if let Some(col_name) = insert_col {
            state.insert_into_active_editor(&col_name);
        }
        if let Some(name) = copy_col_name {
            ui.ctx().copy_text(name.clone());
            state.status_notice = Some(format!("カラム名「{name}」をコピーしました。"));
        }
        if copy_all_cols && !filtered.is_empty() {
            let joined = filtered
                .iter()
                .map(|&idx| state.columns[idx].name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            ui.ctx().copy_text(joined);
            state.status_notice = Some(format!(
                "表示中の全カラム名（{}件）をコピーしました。",
                filtered.len()
            ));
        }
    });
}

/// カラム詳細テーブルのセルクリック・ダブルクリック・右クリックメニューを共通処理する。
fn handle_column_cell_interaction(
    resp: egui::Response,
    orig_idx: usize,
    col_name: &str,
    new_col_selection: &mut Option<usize>,
    insert_col: &mut Option<String>,
    copy_col_name: &mut Option<String>,
    copy_all_cols: &mut bool,
) {
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() || resp.secondary_clicked() {
        *new_col_selection = Some(orig_idx);
    }
    if resp.double_clicked() {
        *insert_col = Some(col_name.to_string());
    }
    resp.context_menu(|ui| {
        if ui.button("カラム名をコピー").clicked() {
            *copy_col_name = Some(col_name.to_string());
            ui.close_menu();
        }
        if ui.button("エディタへカラム名を挿入").clicked() {
            *insert_col = Some(col_name.to_string());
            ui.close_menu();
        }
        ui.separator();
        if ui.button("全カラム名をコピー (カンマ区切り)").clicked() {
            *copy_all_cols = true;
            ui.close_menu();
        }
    });
}

/// カラム一覧から全項目を展開した整形済み SELECT 文を生成する。
pub fn format_full_select_sql(table_name: &str, columns: &[ColumnDetail]) -> String {
    if columns.is_empty() {
        return format!("SELECT * FROM {table_name};\n");
    }
    let cols_block = columns
        .iter()
        .map(|c| format!("    {}", c.name))
        .collect::<Vec<_>>()
        .join(",\n");
    format!("SELECT\n{cols_block}\nFROM {table_name};\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_full_select_with_all_columns() {
        let cols = vec![
            ColumnDetail {
                name: "ID".to_string(),
                data_type: "NUMBER".to_string(),
                length: "10".to_string(),
                nullable: false,
                is_pk: true,
                comments: String::new(),
            },
            ColumnDetail {
                name: "USER_NAME".to_string(),
                data_type: "VARCHAR2".to_string(),
                length: "100".to_string(),
                nullable: true,
                is_pk: false,
                comments: String::new(),
            },
        ];
        let sql = format_full_select_sql("USERS", &cols);
        assert_eq!(sql, "SELECT\n    ID,\n    USER_NAME\nFROM USERS;\n");
    }

    #[test]
    fn falls_back_to_star_when_columns_empty() {
        assert_eq!(
            format_full_select_sql("EMPTY_TBL", &[]),
            "SELECT * FROM EMPTY_TBL;\n"
        );
    }
}
