//! SQL 文の種別判定および危険な DML（WHERE 句なし更新/削除）を検知するモジュール。

use crate::lexer::{TokenKind, tokenize};

/// SQL 文の操作種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    /// 参照系クエリ（SELECT / WITH / SHOW / PRAGMA / EXPLAIN 等）
    Query,
    /// データ操作文（INSERT / UPDATE / DELETE / MERGE / ROLLBACK TO）
    Dml,
    /// データ定義文（CREATE / ALTER / DROP / TRUNCATE / GRANT 等）
    Ddl,
    /// 明示的トランザクション確定（COMMIT）
    Commit,
    /// 明示的トランザクション取消（ROLLBACK）
    Rollback,
    /// その他種別
    Other,
}

/// SQL 文の種別を判定する（先頭のコメント・空白・開き括弧はスキップ）。
pub fn analyze_statement(sql: &str) -> StatementKind {
    let tokens = tokenize(sql);
    let mut significant = tokens.iter().filter(|t| {
        !matches!(t.kind, TokenKind::Whitespace | TokenKind::Comment)
            && &sql[t.range.clone()] != "("
    });

    let Some(first) = significant.next() else {
        return StatementKind::Other;
    };
    let head = sql[first.range.clone()].to_ascii_uppercase();

    match head.as_str() {
        "SELECT" | "WITH" | "SHOW" | "PRAGMA" | "EXPLAIN" => StatementKind::Query,
        "INSERT" | "UPDATE" | "DELETE" | "MERGE" => StatementKind::Dml,
        "CREATE" | "ALTER" | "DROP" | "TRUNCATE" | "RENAME" | "GRANT" | "REVOKE" | "COMMENT" => {
            StatementKind::Ddl
        }
        "COMMIT" => StatementKind::Commit,
        "ROLLBACK" => {
            // `ROLLBACK TO SAVEPOINT ...` は部分ロールバックのため DML 相当として扱う
            if let Some(next) = significant.next()
                && sql[next.range.clone()].eq_ignore_ascii_case("TO")
            {
                return StatementKind::Dml;
            }
            StatementKind::Rollback
        }
        _ => StatementKind::Other,
    }
}

/// `UPDATE` または `DELETE` 文にトップレベル `WHERE` 句が存在しない場合 `true` を返す。
pub fn is_dml_without_where(sql: &str) -> bool {
    let tokens = tokenize(sql);
    let mut depth = 0usize;
    let mut first_kw: Option<String> = None;
    let mut has_top_level_where = false;

    for token in &tokens {
        if matches!(token.kind, TokenKind::Whitespace | TokenKind::Comment) {
            continue;
        }
        let text = &sql[token.range.clone()];
        if text == "(" {
            depth += 1;
            continue;
        }
        if text == ")" {
            depth = depth.saturating_sub(1);
            continue;
        }
        // サブクエリ内の WHERE に誤認しないよう、括弧深度 0 の WHERE のみを検査する
        if depth == 0 && token.kind == TokenKind::Keyword {
            let upper = text.to_ascii_uppercase();
            if first_kw.is_none() {
                first_kw = Some(upper.clone());
            } else if upper == "WHERE" {
                has_top_level_where = true;
                break;
            }
        }
    }

    matches!(first_kw.as_deref(), Some("UPDATE") | Some("DELETE")) && !has_top_level_where
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_statement_kinds() {
        assert_eq!(
            analyze_statement("  -- hi\nSELECT * FROM users"),
            StatementKind::Query
        );
        assert_eq!(
            analyze_statement("UPDATE users SET a = 1 WHERE id = 2"),
            StatementKind::Dml
        );
        assert_eq!(
            analyze_statement("CREATE TABLE t (id INT)"),
            StatementKind::Ddl
        );
        assert_eq!(analyze_statement("COMMIT"), StatementKind::Commit);
        assert_eq!(analyze_statement("ROLLBACK"), StatementKind::Rollback);
        assert_eq!(
            analyze_statement("ROLLBACK TO SAVEPOINT sp1"),
            StatementKind::Dml
        );
        assert_eq!(
            analyze_statement("WITH cte AS (SELECT 1 FROM dual) SELECT * FROM cte"),
            StatementKind::Query
        );
        assert_eq!(
            analyze_statement("MERGE INTO a USING b ON (a.id = b.id) WHEN MATCHED THEN UPDATE SET a.x = 1"),
            StatementKind::Dml
        );
        assert_eq!(
            analyze_statement("TRUNCATE TABLE temp_log"),
            StatementKind::Ddl
        );
        assert_eq!(
            analyze_statement("ALTER TABLE users ADD age NUMBER"),
            StatementKind::Ddl
        );
    }

    #[test]
    fn detects_update_and_delete_without_where() {
        assert!(is_dml_without_where("DELETE FROM users"));
        assert!(is_dml_without_where("UPDATE users SET flag = 1"));
        assert!(is_dml_without_where(
            "UPDATE users SET name = (SELECT name FROM other WHERE id = 1)"
        ));
        assert!(!is_dml_without_where("DELETE FROM users WHERE id = 1"));
        assert!(!is_dml_without_where("SELECT * FROM users"));
    }
}
