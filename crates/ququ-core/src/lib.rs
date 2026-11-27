//! QuQuSQL のコアデータ構造・設定管理・TNS パーサを提供するクレート。
//!
//! UI や DB ドライバから独立した純粋なドメインモデルを定義し、
//! 省メモリなセル表現および接続プロファイルの安全管理を担います。
//!
//! # 使用例
//!
//! ```
//! use compact_str::CompactString;
//! use ququ_core::{CellValue, ColumnMeta, ResultSet};
//!
//! let mut rs = ResultSet::with_columns(vec![
//!     ColumnMeta {
//!         name: CompactString::from("USER_ID"),
//!         data_type: CompactString::from("NUMBER"),
//!         nullable: false,
//!     },
//! ]);
//! rs.push_row(vec![CellValue::Decimal(CompactString::from("9007199254740993"))]);
//!
//! assert_eq!(rs.row_count, 1);
//! assert_eq!(
//!     rs.get(0, 0).unwrap().display_str().as_ref(),
//!     "9007199254740993"
//! );
//! ```

#![warn(missing_docs)]

pub mod export;
pub mod grid_edit;
pub mod metadata;
pub mod profile;
pub mod result_set;
pub mod tns;
pub mod value;

pub use export::{
    export_csv, export_csv_with_header, export_insert_sql, export_tsv, format_sql_literal,
    parse_tsv, quote_identifier,
};
pub use grid_edit::{
    DisplayRowKind, EditableTableData, GridEditOp, GridPatch, InsertedRowSlot, ParameterizedDml,
    RowKey, SelectionStats, SqlDialect, compute_selection_stats, parse_cell_input,
};
pub use metadata::{ColumnDetail, ObjectType, SchemaObject};
pub use profile::{
    AppConfig, ConnectCredentials, ConnectionProfile, DbType, EnvironmentTier, OracleConnectMode,
    StoragePaths, WindowState,
};
pub use result_set::{ColumnMeta, QueryLogEntry, QueryOutcome, ResultSet};
pub use tns::{TnsEntry, find_tnsnames_path, parse_tnsnames};
pub use value::CellValue;
