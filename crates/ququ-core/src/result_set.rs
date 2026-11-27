use crate::CellValue;
use compact_str::CompactString;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMeta {
    pub name: CompactString,
    pub data_type: CompactString,
    pub nullable: bool,
}

/// 1 次元フラット配列でセルを保持する省メモリ結果セット。
#[derive(Debug, Clone)]
pub struct ResultSet {
    pub columns: Vec<ColumnMeta>,
    pub cells: Vec<CellValue>,
    pub row_count: usize,
    pub has_more: bool,
    pub elapsed_ms: u64,
    pub source_sql: Option<String>,
}

impl ResultSet {
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

    pub fn col_count(&self) -> usize {
        self.columns.len()
    }

    pub fn get(&self, row: usize, col: usize) -> Option<&CellValue> {
        let cols = self.columns.len();
        if row >= self.row_count || col >= cols {
            return None;
        }
        self.cells.get(row * cols + col)
    }

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

    /// 追加フェッチした行を結合する。
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

#[derive(Debug, Clone)]
pub enum QueryOutcome {
    Rows(ResultSet),
    Affected { rows: u64, elapsed_ms: u64 },
    Message { text: String, elapsed_ms: u64 },
}

#[derive(Debug, Clone)]
pub struct QueryLogEntry {
    pub timestamp: String,
    pub sql: String,
    pub summary: String,
    pub is_error: bool,
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
