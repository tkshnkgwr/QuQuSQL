use crate::state::{AppAction, UiState};
use crate::theme::AppTheme;
use egui::{Align, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use ququ_core::ObjectType;

pub fn render_object_list_pane(
    ui: &mut Ui,
    state: &mut UiState,
    theme: &AppTheme,
    actions: &mut Vec<AppAction>,
) {
    let is_connected = state.active_profile.is_some();

    ui.vertical(|ui| {
        // owner / type / filter 操作部
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
                            ui.selectable_value(
                                &mut state.selected_owner,
                                owner.clone(),
                                owner,
                            );
                        }
                    });
                if is_connected && state.selected_owner != prev_owner && !state.selected_owner.is_empty() {
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
                if is_connected && state.selected_object_type != prev_type && !state.selected_owner.is_empty() {
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

        // フィルタ適用済みインデックスリスト
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

        TableBuilder::new(ui)
            .id_salt("object_list_table")
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(Align::Center))
            .column(Column::initial(160.0).at_least(80.0).clip(true))
            .column(Column::remainder().at_least(60.0).clip(true))
            .header(22.0, |mut header| {
                header.col(|ui| {
                    ui.label(RichText::new("OBJECT_NAME").strong());
                });
                header.col(|ui| {
                    ui.label(RichText::new("COMMENTS").strong());
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
                        let resp = ui
                            .add(
                                egui::Label::new(RichText::new(&obj.name))
                                    .selectable(false)
                                    .sense(Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if resp.clicked() {
                            new_selection = Some(orig_idx);
                        }
                        if resp.double_clicked() {
                            insert_text = Some(obj.name.clone());
                        }
                        resp.context_menu(|ui| {
                            if ui.button("エディタへ名前挿入").clicked() {
                                insert_text = Some(obj.name.clone());
                                ui.close_menu();
                            }
                            if ui.button("SELECT 文を挿入").clicked() {
                                insert_text = Some(format!(
                                    "SELECT * FROM {};\n",
                                    obj.name
                                ));
                                ui.close_menu();
                            }
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
                        let resp = ui
                            .add(
                                egui::Label::new(&obj.comments)
                                    .selectable(false)
                                    .sense(Sense::click()),
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

        if let Some(idx) = new_selection {
            if state.selected_object_idx != Some(idx) {
                state.selected_object_idx = Some(idx);
                if let Some(obj) = state.objects.get(idx) {
                    actions.push(AppAction::LoadColumns {
                        owner: obj.owner.clone(),
                        table_name: obj.name.clone(),
                    });
                }
            }
        }
        if let Some(snippet) = insert_text {
            insert_into_active_editor(state, &snippet);
        }
        if let Some(txt) = copy_text {
            ui.ctx().copy_text(txt);
        }
    });
}

pub fn render_object_detail_pane(ui: &mut Ui, state: &mut UiState) {
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

        let mut insert_col: Option<String> = None;

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
                    ui.label(RichText::new("column name").strong());
                });
                header.col(|ui| {
                    ui.label(RichText::new("data type").strong());
                });
                header.col(|ui| {
                    ui.label(RichText::new("len").strong());
                });
                header.col(|ui| {
                    ui.label(RichText::new("n").strong());
                });
                header.col(|ui| {
                    ui.label(RichText::new("comments").strong());
                });
            })
            .body(|body| {
                body.rows(20.0, filtered.len(), |mut row| {
                    let col = &state.columns[filtered[row.index()]];
                    row.col(|ui| {
                        let label = if col.is_pk {
                            RichText::new(format!("* {}", col.name)).strong()
                        } else {
                            RichText::new(&col.name)
                        };
                        let resp = ui
                            .add(
                                egui::Label::new(label)
                                    .selectable(false)
                                    .sense(Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if resp.double_clicked() {
                            insert_col = Some(col.name.clone());
                        }
                    });
                    row.col(|ui| {
                        ui.add(egui::Label::new(&col.data_type).selectable(false));
                    });
                    row.col(|ui| {
                        ui.add(egui::Label::new(&col.length).selectable(false));
                    });
                    row.col(|ui| {
                        ui.add(egui::Label::new(if col.nullable { "Y" } else { "N" }).selectable(false));
                    });
                    row.col(|ui| {
                        ui.add(egui::Label::new(&col.comments).selectable(false));
                    });
                });
            });

        if let Some(col_name) = insert_col {
            insert_into_active_editor(state, &col_name);
        }
    });
}

fn insert_into_active_editor(state: &mut UiState, snippet: &str) {
    if let Some(tab) = state.active_tab_mut() {
        let pos = tab.cursor_byte_offset.min(tab.text.len());
        tab.text.insert_str(pos, snippet);
        tab.cursor_byte_offset = pos + snippet.len();
        tab.dirty = true;
    }
}
