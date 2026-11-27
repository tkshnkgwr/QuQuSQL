use crate::state::{AppAction, UiState};
use crate::theme::AppTheme;
use egui::{Align, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use ququ_core::ResultSet;

pub fn render_result_grid(
    ui: &mut Ui,
    state: &mut UiState,
    theme: &AppTheme,
    actions: &mut Vec<AppAction>,
) {
    let Some(rs) = state.result.as_ref() else {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new("SELECT クエリを実行するとここに結果グリッドが表示されます。")
                    .color(theme.null_fg),
            );
        });
        return;
    };

    // グリッド上部バー（件数・時間・TSVコピー・追加取得）
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

        if rs.has_more {
            if let Some(src_sql) = rs.source_sql.clone() {
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
        }

        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            if ui.button("TSV コピー (全行)").clicked() {
                let tsv = result_set_to_tsv(rs, true);
                ui.ctx().copy_text(tsv);
                state.status_notice = Some("結果グリッドを TSV 形式でコピーしました。".to_string());
            }
        });
    });

    ui.separator();

    if rs.columns.is_empty() {
        ui.label("表示する列がありません。");
        return;
    }

    let row_height = 22.0;
    let col_count = rs.col_count();

    let mut table = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(Align::Center))
        .column(Column::exact(48.0)); // 行番号列

    for _ in 0..col_count {
        table = table.column(Column::initial(130.0).at_least(50.0).clip(true));
    }

    let mut cell_buf = String::with_capacity(64);
    let mut clicked_cell: Option<(usize, usize)> = None;
    let mut copy_text: Option<String> = None;

    table
        .header(24.0, |mut header| {
            header.col(|ui| {
                ui.label(RichText::new("#").strong());
            });
            for col in &rs.columns {
                header.col(|ui| {
                    let resp = ui.label(RichText::new(col.name.as_str()).strong());
                    if !col.data_type.is_empty() {
                        resp.on_hover_text(col.data_type.as_str());
                    }
                });
            }
        })
        .body(|body| {
            body.rows(row_height, rs.row_count, |mut row| {
                let row_idx = row.index();
                row.col(|ui| {
                    ui.label(
                        RichText::new(format!("{}", row_idx + 1))
                            .small()
                            .color(theme.editor_gutter_fg),
                    );
                });

                for col_idx in 0..col_count {
                    row.col(|ui| {
                        let Some(val) = rs.get(row_idx, col_idx) else {
                            return;
                        };
                        let is_selected = state.selected_cell == Some((row_idx, col_idx));
                        if is_selected {
                            let rect = ui.max_rect();
                            ui.painter().rect_filled(rect, 0.0, theme.grid_selected_bg);
                        }

                        val.write_display_into(&mut cell_buf);
                        let rich = if val.is_null() {
                            RichText::new(&cell_buf).italics().color(theme.null_fg)
                        } else {
                            RichText::new(&cell_buf)
                        };

                        let resp = ui.add(egui::Label::new(rich).sense(Sense::click()));
                        if resp.clicked() {
                            clicked_cell = Some((row_idx, col_idx));
                        }
                        resp.context_menu(|ui| {
                            if ui.button("このセルをコピー").clicked() {
                                copy_text = Some(val.copy_str().into_owned());
                                ui.close_menu();
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

    if let Some(cell) = clicked_cell {
        state.selected_cell = Some(cell);
    }
    if let Some(txt) = copy_text {
        ui.ctx().copy_text(txt);
    }
}

fn result_set_to_tsv(rs: &ResultSet, with_header: bool) -> String {
    let mut out = String::new();
    if with_header {
        let header = rs
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
            .join("\t");
        out.push_str(&header);
        out.push('\n');
    }

    let cols = rs.col_count();
    for r in 0..rs.row_count {
        for c in 0..cols {
            if c > 0 {
                out.push('\t');
            }
            if let Some(v) = rs.get(r, c) {
                let s = v.copy_str();
                if s.contains(['\t', '\n', '\r', '"']) {
                    out.push('"');
                    out.push_str(&s.replace('"', "\"\""));
                    out.push('"');
                } else {
                    out.push_str(&s);
                }
            }
        }
        out.push('\n');
    }
    out
}
