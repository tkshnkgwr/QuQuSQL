//! SQL の字句解析（シンタックスハイライト用）、文分割、および安全性解析を行うクレート。
//!
//! # 使用例
//!
//! ```
//! use ququ_sql::{StatementKind, analyze_statement, is_dml_without_where, split_statements};
//!
//! let script = "SELECT 'a;b' FROM DUAL; DELETE FROM USERS;";
//! let stmts = split_statements(script);
//! assert_eq!(stmts.len(), 2);
//! assert_eq!(analyze_statement(stmts[0].sql), StatementKind::Query);
//! assert!(is_dml_without_where(stmts[1].sql));
//! ```

#![warn(missing_docs)]

pub mod analyzer;
pub mod lexer;
pub mod splitter;

pub use analyzer::{StatementKind, analyze_statement, is_dml_without_where};
pub use lexer::{Token, TokenKind, tokenize};
pub use splitter::{SqlStatement, split_statements, statement_at_offset};
