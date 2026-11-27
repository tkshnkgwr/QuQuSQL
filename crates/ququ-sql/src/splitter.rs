use crate::lexer::{TokenKind, tokenize};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlStatement<'a> {
    pub sql: &'a str,
    pub range: Range<usize>,
}

/// セミコロン `;` 区切りで SQL 文を分割する（文字列・コメント内の `;` は無視）。
pub fn split_statements(sql: &str) -> Vec<SqlStatement<'_>> {
    let tokens = tokenize(sql);
    let mut statements = Vec::new();
    let mut stmt_start: Option<usize> = None;
    let mut last_non_ws_end: usize = 0;

    for token in &tokens {
        let is_semicolon =
            token.kind == TokenKind::Punctuation && &sql[token.range.clone()] == ";";

        if is_semicolon {
            if let Some(start) = stmt_start.take() {
                let end = last_non_ws_end.max(start);
                let slice = sql[start..end].trim();
                if !slice.is_empty() && !is_only_comments(slice) {
                    statements.push(SqlStatement {
                        sql: slice,
                        range: start..end,
                    });
                }
            }
            continue;
        }

        if !matches!(token.kind, TokenKind::Whitespace | TokenKind::Comment) {
            if stmt_start.is_none() {
                stmt_start = Some(token.range.start);
            }
            last_non_ws_end = token.range.end;
        }
    }

    if let Some(start) = stmt_start {
        let end = last_non_ws_end.max(start);
        let slice = sql[start..end].trim();
        if !slice.is_empty() && !is_only_comments(slice) {
            statements.push(SqlStatement {
                sql: slice,
                range: start..end,
            });
        }
    }

    statements
}

/// カーソル位置（バイトオフセット）を含む 1 文を返す。
/// セミコロンの直後や文末の空白にカーソルがある場合は直前の文を優先する。
pub fn statement_at_offset(sql: &str, byte_offset: usize) -> Option<SqlStatement<'_>> {
    let statements = split_statements(sql);
    if statements.is_empty() {
        return None;
    }

    let clamped = byte_offset.min(sql.len());
    if let Some(found) = statements
        .iter()
        .find(|s| clamped >= s.range.start && clamped <= s.range.end + 1)
    {
        return Some(found.clone());
    }

    // 文と文の間の空白にいる場合、直前の文、なければ直後の文を返す
    statements
        .iter()
        .rfind(|s| s.range.end < clamped)
        .or_else(|| statements.first())
        .cloned()
}

fn is_only_comments(slice: &str) -> bool {
    tokenize(slice)
        .into_iter()
        .all(|t| matches!(t.kind, TokenKind::Whitespace | TokenKind::Comment))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_ignoring_semicolons_in_strings_and_comments() {
        let sql = r#"
-- comment; ignored
SELECT 'a;b' FROM dual;
/* block;
   comment */
UPDATE emp SET name = 'x;y' WHERE id = 1;
"#;
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].sql.contains("SELECT 'a;b' FROM dual"));
        assert!(stmts[1].sql.starts_with("UPDATE emp"));
    }

    #[test]
    fn extracts_statement_at_cursor_offset() {
        let sql = "SELECT 1 FROM dual;\nSELECT 2 FROM dual;";
        let second_pos = sql.find("SELECT 2").unwrap() + 3;
        let stmt = statement_at_offset(sql, second_pos).unwrap();
        assert_eq!(stmt.sql, "SELECT 2 FROM dual");

        let first_end = sql.find(';').unwrap() + 1;
        let stmt1 = statement_at_offset(sql, first_end).unwrap();
        assert_eq!(stmt1.sql, "SELECT 1 FROM dual");
    }
}
