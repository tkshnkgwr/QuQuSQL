//! SQL ソースコードをトークン列へ分解する軽量レキサーモジュール。

use std::ops::Range;

/// SQL トークンの種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// SQL 予約語・組み込み関数名
    Keyword,
    /// テーブル名・列名等の識別子
    Identifier,
    /// シングルクォートで囲まれた文字列リテラル
    StringLiteral,
    /// 数値リテラル
    Number,
    /// 行コメント (`--`) またはブロックコメント (`/* ... */`)
    Comment,
    /// バインド変数 (`:name`)
    BindVar,
    /// 演算子記号
    Operator,
    /// セミコロン・カンマ・括弧等の区切り記号
    Punctuation,
    /// 空白・改行文字
    Whitespace,
}

/// 元の文字列内のバイト範囲を伴う 1 トークン。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// トークン種別
    pub kind: TokenKind,
    /// 元文字列内のバイトオフセット範囲
    pub range: Range<usize>,
}

/// シンタックスハイライト対象の代表的な SQL / Oracle / SQLite キーワード一覧。
const KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "AND", "OR", "NOT", "IN", "IS", "NULL", "AS", "ON", "JOIN",
    "INNER", "LEFT", "RIGHT", "FULL", "OUTER", "CROSS", "ORDER", "BY", "GROUP", "HAVING",
    "ASC", "DESC", "DISTINCT", "UNION", "ALL", "INTERSECT", "MINUS", "EXCEPT", "LIMIT",
    "OFFSET", "FETCH", "FIRST", "NEXT", "ROWS", "ONLY", "INSERT", "INTO", "VALUES", "UPDATE",
    "SET", "DELETE", "MERGE", "USING", "WHEN", "MATCHED", "THEN", "CREATE", "ALTER", "DROP",
    "TRUNCATE", "RENAME", "TABLE", "VIEW", "INDEX", "SEQUENCE", "SYNONYM", "TRIGGER",
    "CONSTRAINT", "PRIMARY", "KEY", "FOREIGN", "REFERENCES", "UNIQUE", "CHECK", "DEFAULT",
    "COMMIT", "ROLLBACK", "SAVEPOINT", "TO", "BEGIN", "END", "DECLARE", "CASE", "ELSE", "LIKE",
    "ESCAPE", "BETWEEN", "EXISTS", "COUNT", "SUM", "AVG", "MIN", "MAX", "NVL", "COALESCE",
    "DECODE", "SYSDATE", "DUAL", "ROWNUM", "ROWID", "WITH", "EXPLAIN", "PLAN", "FOR", "PRAGMA",
    "REPLACE", "IF", "CASCADE", "GRANT", "REVOKE", "COMMENT", "COLUMN", "ADD", "MODIFY",
    "VARCHAR2", "VARCHAR", "NVARCHAR2", "CHAR", "NUMBER", "INTEGER", "INT", "FLOAT", "DATE",
    "TIMESTAMP", "CLOB", "BLOB", "TEXT", "REAL",
];

/// 指定単語が SQL キーワード（大文字・小文字不問）であるかを判定する。
pub fn is_sql_keyword(word: &str) -> bool {
    let upper = word.to_ascii_uppercase();
    KEYWORDS.contains(&upper.as_str())
}

/// SQL 文字列を先頭から走査してトークン列へ分解する。
pub fn tokenize(sql: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let bytes = sql.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let start = i;
        let b = bytes[i];

        // 連続する空白文字
        if b.is_ascii_whitespace() {
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Whitespace,
                range: start..i,
            });
            continue;
        }

        // 行コメント (`-- ...`)
        if b == b'-' && i + 1 < len && bytes[i + 1] == b'-' {
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Comment,
                range: start..i,
            });
            continue;
        }

        // ブロックコメント (`/* ... */`)
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(len);
            tokens.push(Token {
                kind: TokenKind::Comment,
                range: start..i,
            });
            continue;
        }

        // シングルクォート文字列リテラル（`''` エスケープ対応）
        if b == b'\'' {
            i += 1;
            while i < len {
                if bytes[i] == b'\'' {
                    i += 1;
                    if i < len && bytes[i] == b'\'' {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            tokens.push(Token {
                kind: TokenKind::StringLiteral,
                range: start..i,
            });
            continue;
        }

        // ダブルクォート識別子（`""` エスケープ対応）
        if b == b'"' {
            i += 1;
            while i < len {
                if bytes[i] == b'"' {
                    i += 1;
                    if i < len && bytes[i] == b'"' {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Identifier,
                range: start..i,
            });
            continue;
        }

        // バインド変数 (`:name`)
        if b == b':'
            && i + 1 < len
            && (bytes[i + 1].is_ascii_alphanumeric() || bytes[i + 1] == b'_')
        {
            i += 2;
            while i < len
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
            {
                i += 1;
            }
            tokens.push(Token {
                kind: TokenKind::BindVar,
                range: start..i,
            });
            continue;
        }

        // 数値リテラル
        if b.is_ascii_digit() {
            i += 1;
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Number,
                range: start..i,
            });
            continue;
        }

        // 識別子またはキーワード（日本語等の非 ASCII 文字も識別子の一部として扱う）
        if b.is_ascii_alphabetic() || b == b'_' || b >= 0x80 {
            let ch_len = sql[i..].chars().next().map_or(1, char::len_utf8);
            i += ch_len;
            while i < len {
                let nb = bytes[i];
                if nb.is_ascii_alphanumeric() || nb == b'_' || nb == b'$' || nb == b'#' {
                    i += 1;
                } else if nb >= 0x80 {
                    let cl = sql[i..].chars().next().map_or(1, char::len_utf8);
                    i += cl;
                } else {
                    break;
                }
            }
            let word = &sql[start..i];
            let kind = if is_sql_keyword(word) {
                TokenKind::Keyword
            } else {
                TokenKind::Identifier
            };
            tokens.push(Token {
                kind,
                range: start..i,
            });
            continue;
        }

        // 区切り記号
        if matches!(b, b';' | b',' | b'(' | b')' | b'.') {
            i += 1;
            tokens.push(Token {
                kind: TokenKind::Punctuation,
                range: start..i,
            });
            continue;
        }

        // その他演算子・記号
        i += 1;
        tokens.push(Token {
            kind: TokenKind::Operator,
            range: start..i,
        });
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_bind_vars_escaped_strings_and_japanese_identifiers() {
        let sql = "SELECT \"ユーザー名\", 'it''s;ok' /* cmt */ FROM USERS WHERE id = :user_id";
        let tokens = tokenize(sql);
        let kinds_and_text: Vec<(TokenKind, &str)> = tokens
            .iter()
            .filter(|t| t.kind != TokenKind::Whitespace)
            .map(|t| (t.kind, &sql[t.range.clone()]))
            .collect();

        assert!(kinds_and_text.contains(&(TokenKind::Keyword, "SELECT")));
        assert!(kinds_and_text.contains(&(TokenKind::Identifier, "\"ユーザー名\"")));
        assert!(kinds_and_text.contains(&(TokenKind::StringLiteral, "'it''s;ok'")));
        assert!(kinds_and_text.contains(&(TokenKind::Comment, "/* cmt */")));
        assert!(kinds_and_text.contains(&(TokenKind::BindVar, ":user_id")));
    }
}
