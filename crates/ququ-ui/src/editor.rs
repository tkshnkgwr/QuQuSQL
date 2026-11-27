use crate::state::{FindReplaceState, SqlTab};
use crate::theme::AppTheme;
use egui::text::{CCursor, CCursorRange, LayoutJob};
use egui::{Align, Color32, FontFamily, FontId, RichText, TextFormat, Ui};
use ququ_sql::{TokenKind, tokenize};
use regex::{Regex, RegexBuilder};

pub fn render_editor_pane(
    ui: &mut Ui,
    tab: &mut SqlTab,
    find_replace: &mut FindReplaceState,
    theme: &AppTheme,
    font_size: f32,
) {
    // 検索・置換パネル（必須機能）
    if find_replace.open {
        render_find_replace_bar(ui, tab, find_replace);
        ui.separator();
    }

    let editor_id = ui.make_persistent_id(format!("sql_editor_{}", tab.id));

    // 検索ジャンプ等で予約された選択範囲を反映
    if let Some((start_b, end_b)) = find_replace.pending_select_range.take() {
        let c_start = byte_to_char_idx(&tab.text, start_b);
        let c_end = byte_to_char_idx(&tab.text, end_b);
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), editor_id) {
            state
                .cursor
                .set_char_range(Some(CCursorRange::two(CCursor::new(c_start), CCursor::new(c_end))));
            state.store(ui.ctx(), editor_id);
        }
        ui.memory_mut(|m| m.request_focus(editor_id));
    }

    let font_id = FontId::new(font_size, FontFamily::Monospace);
    let line_count = tab.text.split('\n').count().max(1);
    let gutter_digits = line_count.to_string().len().max(3);
    let gutter_width = (gutter_digits as f32) * (font_size * 0.65) + 14.0;

    egui::Frame::NONE
        .fill(theme.editor_bg)
        .show(ui, |ui| {
            egui::ScrollArea::both()
                .id_salt(format!("editor_scroll_{}", tab.id))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        // 左ガター：行番号
                        let mut line_nums = String::with_capacity(line_count * 5);
                        for idx in 1..=line_count {
                            if idx > 1 {
                                line_nums.push('\n');
                            }
                            line_nums.push_str(&format!("{idx:>gutter_digits$}"));
                        }

                        egui::Frame::NONE
                            .fill(theme.editor_gutter_bg)
                            .inner_margin(egui::Margin::symmetric(6, 4))
                            .show(ui, |ui| {
                                ui.set_min_width(gutter_width);
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(line_nums)
                                            .font(font_id.clone())
                                            .color(theme.editor_gutter_fg),
                                    )
                                    .selectable(false),
                                );
                            });

                        // SQL シンタックスハイライト Layouter
                        let theme_clone = theme.clone();
                        let font_clone = font_id.clone();
                        let mut layouter = |ui: &Ui, text: &str, wrap_width: f32| {
                            let mut job = build_highlight_job(text, &theme_clone, font_clone.clone());
                            job.wrap.max_width = wrap_width;
                            ui.fonts(|f| f.layout_job(job))
                        };

                        let output = egui::TextEdit::multiline(&mut tab.text)
                            .id(editor_id)
                            .font(font_id)
                            .code_editor()
                            .frame(false)
                            .desired_width(f32::INFINITY)
                            .desired_rows(20)
                            .lock_focus(true)
                            .layouter(&mut layouter)
                            .show(ui);

                        if output.response.changed() {
                            tab.dirty = true;
                        }

                        if let Some(range) = output.cursor_range {
                            let primary = range.primary.ccursor.index;
                            let secondary = range.secondary.ccursor.index;
                            let byte_off = char_to_byte_idx(&tab.text, primary);
                            tab.cursor_byte_offset = byte_off;
                            let (line, col) = offset_to_line_col(&tab.text, byte_off);
                            tab.cursor_line = line;
                            tab.cursor_col = col;

                            if primary != secondary {
                                let a = char_to_byte_idx(&tab.text, primary.min(secondary));
                                let b = char_to_byte_idx(&tab.text, primary.max(secondary));
                                tab.selection_byte_range = Some((a, b));
                            } else {
                                tab.selection_byte_range = None;
                            }
                        }
                    });
                });
        });
}

fn render_find_replace_bar(ui: &mut Ui, tab: &mut SqlTab, state: &mut FindReplaceState) {
    let matches = find_all_matches(&tab.text, state);
    let match_count = matches.len();

    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("検索:");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut state.query)
                        .desired_width(220.0)
                        .hint_text("検索文字列..."),
                );

                let enter_next =
                    resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                if ui.button("▲ 前へ").clicked() {
                    jump_to_match(tab, state, &matches, false);
                }
                if ui.button("▼ 次へ").clicked() || enter_next {
                    jump_to_match(tab, state, &matches, true);
                }

                ui.checkbox(&mut state.case_sensitive, "大小区別");
                ui.checkbox(&mut state.use_regex, "正規表現");
                ui.checkbox(&mut state.show_replace, "置換");

                if !state.query.is_empty() {
                    ui.label(
                        RichText::new(format!("{match_count} 件"))
                            .small()
                            .color(if match_count == 0 {
                                Color32::from_rgb(200, 60, 60)
                            } else {
                                Color32::GRAY
                            }),
                    );
                }

                if !state.status_msg.is_empty() {
                    ui.label(RichText::new(&state.status_msg).small().color(Color32::GRAY));
                }

                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("✕").clicked() {
                        state.open = false;
                        state.status_msg.clear();
                    }
                });
            });

            if state.show_replace {
                ui.horizontal(|ui| {
                    ui.label("置換:");
                    ui.add(
                        egui::TextEdit::singleline(&mut state.replace_with)
                            .desired_width(220.0)
                            .hint_text("置換後文字列..."),
                    );

                    if ui.button("1件置換").clicked() {
                        replace_current_or_next(tab, state, &matches);
                    }
                    if ui.button("すべて置換").clicked() {
                        replace_all_matches(tab, state, &matches);
                    }
                });
            }
        });
}

fn build_highlight_job(text: &str, theme: &AppTheme, font_id: FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    let tokens = tokenize(text);
    for tok in tokens {
        let slice = &text[tok.range];
        let color = match tok.kind {
            TokenKind::Keyword => theme.keyword,
            TokenKind::StringLiteral => theme.string_lit,
            TokenKind::Number => theme.number_lit,
            TokenKind::Comment => theme.comment,
            TokenKind::BindVar => theme.bind_var,
            TokenKind::Operator | TokenKind::Punctuation => theme.operator,
            TokenKind::Identifier | TokenKind::Whitespace => theme.identifier,
        };
        job.append(
            slice,
            0.0,
            TextFormat {
                font_id: font_id.clone(),
                color,
                ..Default::default()
            },
        );
    }
    job
}

fn compile_search_regex(state: &FindReplaceState) -> Option<Regex> {
    if state.query.is_empty() {
        return None;
    }
    let pattern = if state.use_regex {
        state.query.clone()
    } else {
        regex::escape(&state.query)
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!state.case_sensitive)
        .build()
        .ok()
}

fn find_all_matches(text: &str, state: &FindReplaceState) -> Vec<(usize, usize)> {
    let Some(re) = compile_search_regex(state) else {
        return Vec::new();
    };
    re.find_iter(text).map(|m| (m.start(), m.end())).collect()
}

fn jump_to_match(
    tab: &SqlTab,
    state: &mut FindReplaceState,
    matches: &[(usize, usize)],
    forward: bool,
) {
    if matches.is_empty() {
        state.status_msg = "一致なし".to_string();
        return;
    }
    let cur = tab.cursor_byte_offset;
    let target = if forward {
        matches
            .iter()
            .find(|(s, _)| *s > cur)
            .or_else(|| matches.first())
    } else {
        matches
            .iter()
            .rfind(|(_, e)| *e < cur)
            .or_else(|| matches.last())
    };
    if let Some(&(s, e)) = target {
        state.pending_select_range = Some((s, e));
        state.status_msg.clear();
    }
}

fn replace_current_or_next(
    tab: &mut SqlTab,
    state: &mut FindReplaceState,
    matches: &[(usize, usize)],
) {
    if matches.is_empty() {
        state.status_msg = "一致なし".to_string();
        return;
    }

    let chosen = tab
        .selection_byte_range
        .and_then(|sel| matches.iter().copied().find(|m| *m == sel))
        .or_else(|| {
            matches
                .iter()
                .copied()
                .find(|(s, _)| *s >= tab.cursor_byte_offset)
                .or_else(|| matches.first().copied())
        });

    if let Some((s, e)) = chosen {
        tab.text.replace_range(s..e, &state.replace_with);
        tab.dirty = true;
        let new_end = s + state.replace_with.len();
        tab.cursor_byte_offset = new_end;
        state.status_msg = "1件置換しました".to_string();

        let next_matches = find_all_matches(&tab.text, state);
        if let Some(&(ns, ne)) = next_matches
            .iter()
            .find(|(ms, _)| *ms >= new_end)
            .or_else(|| next_matches.first())
        {
            state.pending_select_range = Some((ns, ne));
        }
    }
}

fn replace_all_matches(
    tab: &mut SqlTab,
    state: &mut FindReplaceState,
    matches: &[(usize, usize)],
) {
    if matches.is_empty() {
        state.status_msg = "一致なし".to_string();
        return;
    }
    let count = matches.len();
    for &(s, e) in matches.iter().rev() {
        tab.text.replace_range(s..e, &state.replace_with);
    }
    tab.dirty = true;
    state.status_msg = format!("{count} 件置換しました");
}

fn byte_to_char_idx(s: &str, byte_idx: usize) -> usize {
    let clamped = byte_idx.min(s.len());
    s[..clamped].chars().count()
}

fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map_or(s.len(), |(byte_i, _)| byte_i)
}

fn offset_to_line_col(s: &str, byte_offset: usize) -> (usize, usize) {
    let prefix = &s[..byte_offset.min(s.len())];
    let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
    let last_nl = prefix.rfind('\n').map_or(0, |idx| idx + 1);
    let col = prefix[last_nl..].chars().count() + 1;
    (line, col)
}
