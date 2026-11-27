//! グリッドデータのクリップボード貼付（Excel 互換 TSV）およびファイル出力モジュール。
//!
//! CSV / TSV / INSERT 文のエクスポートと、改行やダブルクォートを含む
//! Excel 形式の TSV パースを提供する。

use std::fmt::Write;

use crate::{CellValue, ColumnMeta, SqlDialect};

/// Excel 互換の TSV テキストを行・列の 2 次元文字列配列に分解する。
///
/// ダブルクォートで囲まれたセル内改行や `""` エスケープ、CRLF / LF / CR 改行に対応し、
/// 末尾の余分な改行 1 つ分は空行として扱わない。
pub fn parse_tsv(text: &str) -> Vec<Vec<String>> {
    // 末尾の単独改行を除去（Excel のコピー時は末尾に \r\n が付く）
    let normalized = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .or_else(|| text.strip_suffix('\r'))
        .unwrap_or(text);

    if normalized.is_empty() {
        return Vec::new();
    }

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_cell = String::new();
    let mut in_quotes = false;
    let mut at_cell_start = true;
    let mut chars = normalized.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    current_cell.push('"');
                } else {
                    in_quotes = false;
                }
            } else if ch == '\r' {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                current_cell.push('\n');
            } else {
                current_cell.push(ch);
            }
            continue;
        }

        if at_cell_start && ch == '"' {
            in_quotes = true;
            at_cell_start = false;
            continue;
        }

        match ch {
            '\t' => {
                current_row.push(std::mem::take(&mut current_cell));
                at_cell_start = true;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                current_row.push(std::mem::take(&mut current_cell));
                rows.push(std::mem::take(&mut current_row));
                at_cell_start = true;
            }
            '\n' => {
                current_row.push(std::mem::take(&mut current_cell));
                rows.push(std::mem::take(&mut current_row));
                at_cell_start = true;
            }
            other => {
                current_cell.push(other);
                at_cell_start = false;
            }
        }
    }

    current_row.push(current_cell);
    rows.push(current_row);
    rows
}

/// CSV 文字列としてセル 1 つ分をフォーマットする（RFC 4180 準拠）。
fn push_csv_field(out: &mut String, val: &CellValue) {
    if matches!(val, CellValue::Null) {
        return;
    }
    let s = val.display_str();
    if s.contains([',', '"', '\n', '\r']) {
        out.push('"');
        for ch in s.chars() {
            if ch == '"' {
                out.push('"');
            }
            out.push(ch);
        }
        out.push('"');
    } else {
        out.push_str(&s);
    }
}

/// TSV 文字列としてセル 1 つ分をフォーマットする（Excel 互換クォート）。
fn push_tsv_field(out: &mut String, val: &CellValue) {
    if matches!(val, CellValue::Null) {
        return;
    }
    let s = val.display_str();
    if s.contains(['\t', '"', '\n', '\r']) {
        out.push('"');
        for ch in s.chars() {
            if ch == '"' {
                out.push('"');
            }
            out.push(ch);
        }
        out.push('"');
    } else {
        out.push_str(&s);
    }
}

/// カラム定義と行データから CSV 文字列を生成する（ヘッダ行あり）。
pub fn export_csv<'a, R>(columns: &[ColumnMeta], rows: R) -> String
where
    R: IntoIterator<Item = Vec<&'a CellValue>>,
{
    export_csv_with_header(columns, rows, true)
}

/// カラム定義と行データから CSV 文字列を生成する（ヘッダ行出力の有無を選択可能）。
pub fn export_csv_with_header<'a, R>(
    columns: &[ColumnMeta],
    rows: R,
    include_header: bool,
) -> String
where
    R: IntoIterator<Item = Vec<&'a CellValue>>,
{
    let mut out = String::new();
    if include_header {
        for (idx, col) in columns.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            let header_val = CellValue::Text(col.name.as_str().into());
            push_csv_field(&mut out, &header_val);
        }
        out.push('\n');
    }

    for row in rows {
        for (idx, cell) in row.into_iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            push_csv_field(&mut out, cell);
        }
        out.push('\n');
    }
    out
}

/// カラム定義と行データから TSV 文字列を生成する（ヘッダ出力の有無を選択可能）。
pub fn export_tsv<'a, R>(columns: &[ColumnMeta], rows: R, include_header: bool) -> String
where
    R: IntoIterator<Item = Vec<&'a CellValue>>,
{
    let mut out = String::new();
    if include_header {
        for (idx, col) in columns.iter().enumerate() {
            if idx > 0 {
                out.push('\t');
            }
            let header_val = CellValue::Text(col.name.as_str().into());
            push_tsv_field(&mut out, &header_val);
        }
        out.push('\n');
    }

    for row in rows {
        for (idx, cell) in row.into_iter().enumerate() {
            if idx > 0 {
                out.push('\t');
            }
            push_tsv_field(&mut out, cell);
        }
        out.push('\n');
    }
    out
}

/// SQL リテラル表現へ整形する（プレビュー表示および INSERT 文エクスポート用）。
pub fn format_sql_literal(val: &CellValue, dialect: SqlDialect) -> String {
    match val {
        CellValue::Null => "NULL".to_string(),
        CellValue::Bool(b) => {
            if *b {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        CellValue::Int(n) => n.to_string(),
        CellValue::Float(f) => {
            if f.is_finite() {
                f.to_string()
            } else {
                "NULL".to_string()
            }
        }
        CellValue::Decimal(s) => {
            let trimmed = s.trim();
            if trimmed.parse::<f64>().is_ok() {
                trimmed.to_string()
            } else {
                quote_sql_string(trimmed)
            }
        }
        CellValue::Text(s) | CellValue::DateTime(s) => quote_sql_string(s),
        CellValue::Bytes(bytes) => {
            let mut hex = String::with_capacity(bytes.len() * 2);
            for b in bytes.iter() {
                let _ = write!(hex, "{b:02X}");
            }
            match dialect {
                SqlDialect::Sqlite => format!("X'{hex}'"),
                SqlDialect::Oracle => format!("HEXTORAW('{hex}')"),
            }
        }
    }
}

/// SQL 識別子（テーブル名・列名）を必要最小限にクォートする。
pub fn quote_identifier(ident: &str, dialect: SqlDialect) -> String {
    let clean = ident.trim();
    let is_simple = !clean.is_empty()
        && clean
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_');
    if is_simple && !clean.starts_with(|c: char| c.is_ascii_digit()) {
        clean.to_string()
    } else {
        let escaped = clean.replace('"', "\"\"");
        match dialect {
            SqlDialect::Sqlite | SqlDialect::Oracle => format!("\"{escaped}\""),
        }
    }
}

/// カラム定義と行データから `INSERT INTO ... VALUES (...);` 群を生成する。
pub fn export_insert_sql<'a, R>(
    table_name: &str,
    columns: &[ColumnMeta],
    rows: R,
    dialect: SqlDialect,
) -> String
where
    R: IntoIterator<Item = Vec<&'a CellValue>>,
{
    let quoted_table = quote_identifier(table_name, dialect);
    let col_list = columns
        .iter()
        .map(|c| quote_identifier(&c.name, dialect))
        .collect::<Vec<_>>()
        .join(", ");

    let mut out = String::new();
    for row in rows {
        let vals = row
            .into_iter()
            .map(|cell| format_sql_literal(cell, dialect))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "INSERT INTO {quoted_table} ({col_list}) VALUES ({vals});");
    }
    out
}

/// シングルクォートで SQL 文字列リテラルをエスケープする。
fn quote_sql_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tsv_handles_excel_quotes_and_newlines() {
        let tsv = "1\t\"hello\r\nworld\"\t\"a\"\"b\"\r\n2\tplain\t\r\n";
        let parsed = parse_tsv(tsv);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], vec!["1", "hello\nworld", "a\"b"]);
        assert_eq!(parsed[1], vec!["2", "plain", ""]);
    }

    #[test]
    fn export_csv_and_insert_sql_escape_correctly() {
        let cols = vec![
            ColumnMeta {
                name: "ID".into(),
                data_type: "INTEGER".into(),
                nullable: false,
            },
            ColumnMeta {
                name: "NOTE".into(),
                data_type: "TEXT".into(),
                nullable: true,
            },
        ];
        let row1 = [CellValue::Int(1), CellValue::Text("O'Reilly, Inc.".into())];
        let row2 = [CellValue::Int(2), CellValue::Null];

        let csv = export_csv(&cols, [
            row1.iter().collect::<Vec<_>>(),
            row2.iter().collect::<Vec<_>>(),
        ]);
        assert_eq!(csv, "ID,NOTE\n1,\"O'Reilly, Inc.\"\n2,\n");

        let sql = export_insert_sql(
            "USERS",
            &cols,
            [
                row1.iter().collect::<Vec<_>>(),
                row2.iter().collect::<Vec<_>>(),
            ],
            SqlDialect::Oracle,
        );
        assert_eq!(
            sql,
            "INSERT INTO USERS (ID, NOTE) VALUES (1, 'O''Reilly, Inc.');\nINSERT INTO USERS (ID, NOTE) VALUES (2, NULL);\n"
        );
    }
}
