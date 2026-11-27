//! SQL 実行結果セットおよび実行ログの型定義モジュール。

use crate::CellValue;
use compact_str::CompactString;

/// 結果セットの列メタデータ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMeta {
    /// 列名
    pub name: CompactString,
    /// DB 上のデータ型名
    pub data_type: CompactString,
    /// NULL 許可フラグ
    pub nullable: bool,
}

/// 1 次元フラット配列でセルを保持する省メモリ結果セット。
#[derive(Debug, Clone)]
pub struct ResultSet {
    /// 列メタデータ一覧
    pub columns: Vec<ColumnMeta>,
    /// 行優先の 1 次元配列（`row * col_count + col` で参照）
    pub cells: Vec<CellValue>,
    /// 現在の保持行数
    pub row_count: usize,
    /// フェッチ上限により未取得の後続行が残っているか
    pub has_more: bool,
    /// クエリ実行およびフェッチにかかった累計時間（ミリ秒）
    pub elapsed_ms: u64,
    /// 追加フェッチ用に保持する元の SELECT 文
    pub source_sql: Option<String>,
}

impl ResultSet {
    /// 空の結果セットを生成する。
    pub fn empty(elapsed_ms: u64) -> Self {
        Self {
            columns: Vec::new(),
            cells: Vec::new(),
            row_count: 0,
            has_more: false,
            elapsed_ms,
            source_sql: None,
        }
    }

    /// 列メタデータのみを指定して空の結果セットを初期化する。
    pub fn with_columns(columns: Vec<ColumnMeta>) -> Self {
        Self {
            columns,
            cells: Vec::new(),
            row_count: 0,
            has_more: false,
            elapsed_ms: 0,
            source_sql: None,
        }
    }

    /// 列数を返す。
    pub fn col_count(&self) -> usize {
        self.columns.len()
    }

    /// 指定行・指定列のセル参照を返す。範囲外の場合は `None`。
    pub fn get(&self, row: usize, col: usize) -> Option<&CellValue> {
        let cols = self.columns.len();
        if row >= self.row_count || col >= cols {
            return None;
        }
        self.cells.get(row * cols + col)
    }

    /// 1 行分のセル配列を末尾へ追加する（列数が不足する場合は NULL 埋めする）。
    pub fn push_row(&mut self, mut row: Vec<CellValue>) {
        let cols = self.columns.len();
        if row.len() < cols {
            row.resize(cols, CellValue::Null);
        } else if row.len() > cols {
            row.truncate(cols);
        }
        self.cells.extend(row);
        self.row_count += 1;
    }

    /// 追加フェッチした行を既存の結果セット末尾へ結合する。
    pub fn append(&mut self, more: ResultSet) {
        if self.columns.is_empty() {
            *self = more;
            return;
        }
        if more.columns.len() != self.columns.len() {
            return;
        }
        self.cells.extend(more.cells);
        self.row_count += more.row_count;
        self.has_more = more.has_more;
        self.elapsed_ms += more.elapsed_ms;
    }
}

/// SQL 実行結果の種別。
#[derive(Debug, Clone)]
pub enum QueryOutcome {
    /// SELECT 等で行セットを返した場合
    Rows(ResultSet),
    /// DML（INSERT / UPDATE / DELETE 等）で影響行数を返した場合
    Affected {
        /// 変更・削除・挿入された行数
        rows: u64,
        /// 実行時間（ミリ秒）
        elapsed_ms: u64,
    },
    /// DDL や COMMIT / ROLLBACK 等の完了メッセージ
    Message {
        /// 完了メッセージ本文
        text: String,
        /// 実行時間（ミリ秒）
        elapsed_ms: u64,
    },
}

/// ログペインに表示する 1 回分の実行履歴エントリ。
#[derive(Debug, Clone)]
pub struct QueryLogEntry {
    /// 実行時刻文字列
    pub timestamp: String,
    /// 実行した SQL または操作コンテキスト
    pub sql: String,
    /// 処理件数やエラー内容のサマリ
    pub summary: String,
    /// エラー発生エントリかどうか
    pub is_error: bool,
    /// 実行時間（ミリ秒）
    pub elapsed_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_result_set_access_and_append() {
        let cols = vec![
            ColumnMeta {
                name: CompactString::from("ID"),
                data_type: CompactString::from("INTEGER"),
                nullable: false,
            },
            ColumnMeta {
                name: CompactString::from("NAME"),
                data_type: CompactString::from("TEXT"),
                nullable: true,
            },
        ];

        let mut rs = ResultSet::with_columns(cols.clone());
        rs.push_row(vec![
            CellValue::Int(1),
            CellValue::Text(CompactString::from("Alice")),
        ]);
        rs.push_row(vec![CellValue::Int(2), CellValue::Null]);
        rs.has_more = true;

        assert_eq!(rs.row_count, 2);
        assert_eq!(rs.get(0, 0), Some(&CellValue::Int(1)));
        assert_eq!(rs.get(1, 1), Some(&CellValue::Null));
        assert_eq!(rs.get(2, 0), None);

        let mut rs2 = ResultSet::with_columns(cols);
        rs2.push_row(vec![
            CellValue::Int(3),
            CellValue::Text(CompactString::from("Bob")),
        ]);
        rs2.has_more = false;

        rs.append(rs2);
        assert_eq!(rs.row_count, 3);
        assert!(!rs.has_more);
        assert_eq!(
            rs.get(2, 1),
            Some(&CellValue::Text(CompactString::from("Bob")))
        );
    }
}
