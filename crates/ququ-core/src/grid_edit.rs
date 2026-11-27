//! OSqlGrid 向けのセル・行編集モデル、差分パッチ、および選択統計モジュール。
//!
//! DB バックエンドや GUI フレームワークから独立した純粋なデータ構造として、
//! 行識別キー（ROWID / 主キー）、Undo/Redo 可能なパッチ履歴（`GridPatch`）、
//! および選択範囲の即時集計（`SelectionStats`）を提供する。

use std::collections::{BTreeMap, BTreeSet};

use compact_str::CompactString;

use crate::{CellValue, ColumnMeta, ResultSet};

/// 対象データベースの SQL 方言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlDialect {
    /// SQLite 方言（`?1, ?2, ...` バインドおよび `rowid` 識別）。
    Sqlite,
    /// Oracle Database 方言（`:1, :2, ...` バインドおよび `ROWID` 識別）。
    Oracle,
}

/// グリッド上の既存行・新規行を一意に識別するためのキー。
#[derive(Debug, Clone, PartialEq)]
pub enum RowKey {
    /// SQLite の `rowid` または Oracle の `ROWIDTOCHAR(ROWID)` 文字列。
    RowId(String),
    /// 主キー列名と値のペアによる識別キー。
    PrimaryKey(Vec<(String, CellValue)>),
    /// まだ DB へ INSERT されていない新規行の一時識別子。
    NewRow(u64),
}

/// OSqlGrid で直接編集するためのテーブル読み込み結果。
#[derive(Debug, Clone)]
pub struct EditableTableData {
    /// 接続先 SQL 方言。
    pub dialect: SqlDialect,
    /// スキーマ（オーナー）名。
    pub owner: String,
    /// 対象テーブル名。
    pub table_name: String,
    /// 適用中の WHERE 句（空文字なら未指定）。
    pub where_clause: String,
    /// 適用中の ORDER BY 句（空文字なら未指定）。
    pub order_by: String,
    /// テーブルのカラム定義メタデータ。
    pub columns: Vec<ColumnMeta>,
    /// 各既存行の識別キー（ビュー等で取得不可の行は `None`）。
    pub row_keys: Vec<Option<RowKey>>,
    /// 取得したセルデータの本体。
    pub result: ResultSet,
}

impl EditableTableData {
    /// 既存の DB 行がすべて編集可能な識別キー（ROWID 等）を持っているかを返す。
    pub fn all_rows_editable(&self) -> bool {
        !self.columns.is_empty() && self.row_keys.iter().all(Option::is_some)
    }

    /// 指定した既存行が UPDATE / DELETE 用の識別キーを持っているかを返す。
    pub fn can_edit_existing_row(&self, row: usize) -> bool {
        self.row_keys.get(row).is_some_and(Option::is_some)
    }
}

/// パラメータ化された 1 件の DML 実行命令。
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterizedDml {
    /// バインドプレースホルダ付きの実行 SQL 文。
    pub sql: String,
    /// バインドパラメータ値の配列。
    pub params: Vec<CellValue>,
    /// 確認ダイアログ表示用のリテラル展開済みプレビュー SQL（実行には使用しない）。
    pub preview_sql: String,
    /// 影響行数が厳密に 1 行であることを検証するか（UPDATE / DELETE で `true`）。
    pub expect_one_row: bool,
}

/// グリッド上で記録される単一または複合の編集操作（Undo/Redo 用）。
#[derive(Debug, Clone, PartialEq)]
pub enum GridEditOp {
    /// 既存行または新規行の単一セル値変更。
    UpdateCell {
        /// 論理行インデックス（`0..base_row_count` は既存行、それ以降は新規行スロット）。
        row: usize,
        /// 列インデックス。
        col: usize,
        /// 変更前のセル値。
        old_val: CellValue,
        /// 変更後のセル値。
        new_val: CellValue,
    },
    /// 新規行の追加（または追加スロットの有効化）。
    InsertRow {
        /// 新規行スロットに対応する論理行インデックス。
        row: usize,
        /// 初期セル値配列。
        values: Vec<CellValue>,
    },
    /// 新規行スロットの表示無効化（未反映の新規行を削除した操作）。
    DeactivateInsertedRow {
        /// 論理行インデックス。
        row: usize,
    },
    /// 既存行に対する削除マークのトグル。
    ToggleDeleteRow {
        /// 既存行の論理行インデックス。
        row: usize,
        /// 操作前に削除マークが付いていたか。
        deleted_before: bool,
    },
    /// 複数セル貼付や複数行削除などを 1 ステップとして扱う複合操作。
    Batch(Vec<GridEditOp>),
}

/// 新規追加行の内部スロット。
#[derive(Debug, Clone, PartialEq)]
pub struct InsertedRowSlot {
    /// この行の各列の現在値。
    pub values: Vec<CellValue>,
    /// グリッド上および INSERT 対象として有効かどうか（Undo 整合性のため論理保持する）。
    pub active: bool,
}

/// グリッド上の表示行と論理行のマッピング種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayRowKind {
    /// DB から取得した既存行（`logical_row` は `0..base_row_count`）。
    Existing {
        /// `EditableTableData::result` 内の行インデックス。
        logical_row: usize,
        /// 削除マークが付与されているか。
        marked_deleted: bool,
    },
    /// 未反映の新規追加行（`logical_row` は `base_row_count + slot_idx`）。
    Inserted {
        /// パッチ内の論理行インデックス。
        logical_row: usize,
    },
}

/// テーブル全体に対する未反映の変更差分と Undo/Redo 履歴。
#[derive(Debug, Clone)]
pub struct GridPatch {
    /// 読み込んだ時点の既存行数。
    base_row_count: usize,
    /// 列数。
    col_count: usize,
    /// 既存行の編集済みセル（元データと異なるセルのみ保持）。
    cell_edits: BTreeMap<(usize, usize), CellValue>,
    /// 削除マークされた既存行インデックスの集合。
    deleted_rows: BTreeSet<usize>,
    /// 追加された新規行スロット一覧。
    inserted_rows: Vec<InsertedRowSlot>,
    /// Undo スタック。
    undo_stack: Vec<GridEditOp>,
    /// Redo スタック。
    redo_stack: Vec<GridEditOp>,
}

impl GridPatch {
    /// 指定された既存行数・列数に対応する空の差分パッチを作成する。
    pub fn new(base_row_count: usize, col_count: usize) -> Self {
        Self {
            base_row_count,
            col_count,
            cell_edits: BTreeMap::new(),
            deleted_rows: BTreeSet::new(),
            inserted_rows: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// 未反映の変更（セル変更・削除マーク・有効な新規追加行）が 1 件以上あるかを返す。
    pub fn has_changes(&self) -> bool {
        !self.cell_edits.is_empty()
            || !self.deleted_rows.is_empty()
            || self.inserted_rows.iter().any(|slot| slot.active)
    }

    /// 変更された既存行の行数（同じ行で複数列を変更しても 1 行として数える）を返す。
    pub fn modified_existing_row_count(&self) -> usize {
        self.cell_edits
            .keys()
            .map(|(r, _)| *r)
            .filter(|r| !self.deleted_rows.contains(r))
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// 削除マークされた既存行の件数を返す。
    pub fn deleted_row_count(&self) -> usize {
        self.deleted_rows.len()
    }

    /// 有効な新規追加行の件数を返す。
    pub fn inserted_row_count(&self) -> usize {
        self.inserted_rows.iter().filter(|s| s.active).count()
    }

    /// Undo 可能かを返す。
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Redo 可能かを返す。
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// 読み込んだ時点の既存行数を返す。
    pub fn base_row_count(&self) -> usize {
        self.base_row_count
    }

    /// 現在のグリッドに表示すべき全行のメタ情報一覧を返す。
    pub fn display_rows(&self) -> Vec<DisplayRowKind> {
        let mut rows = Vec::with_capacity(self.base_row_count + self.inserted_rows.len());
        for r in 0..self.base_row_count {
            rows.push(DisplayRowKind::Existing {
                logical_row: r,
                marked_deleted: self.deleted_rows.contains(&r),
            });
        }
        for (idx, slot) in self.inserted_rows.iter().enumerate() {
            if slot.active {
                rows.push(DisplayRowKind::Inserted {
                    logical_row: self.base_row_count + idx,
                });
            }
        }
        rows
    }

    /// 指定した論理行・列の現在表示すべきセル値を返す。
    pub fn cell_value<'a>(
        &'a self,
        table: &'a EditableTableData,
        logical_row: usize,
        col: usize,
    ) -> &'a CellValue {
        if logical_row < self.base_row_count {
            if let Some(edited) = self.cell_edits.get(&(logical_row, col)) {
                return edited;
            }
            return table
                .result
                .get(logical_row, col)
                .unwrap_or(&CellValue::Null);
        }
        let slot_idx = logical_row - self.base_row_count;
        self.inserted_rows
            .get(slot_idx)
            .and_then(|s| s.values.get(col))
            .unwrap_or(&CellValue::Null)
    }

    /// 指定セルが元データから変更されているかを返す。
    pub fn is_cell_modified(&self, logical_row: usize, col: usize) -> bool {
        logical_row < self.base_row_count && self.cell_edits.contains_key(&(logical_row, col))
    }

    /// 指定した既存行が削除マークされているかを返す。
    pub fn is_row_deleted(&self, logical_row: usize) -> bool {
        logical_row < self.base_row_count && self.deleted_rows.contains(&logical_row)
    }

    /// 既存行ごとの「変更された列番号と新値」の一覧を返す（最小列 UPDATE 生成用）。
    pub fn modified_cells_by_row(&self) -> BTreeMap<usize, Vec<(usize, CellValue)>> {
        let mut grouped: BTreeMap<usize, Vec<(usize, CellValue)>> = BTreeMap::new();
        for (&(row, col), val) in &self.cell_edits {
            if self.deleted_rows.contains(&row) {
                continue;
            }
            grouped.entry(row).or_default().push((col, val.clone()));
        }
        grouped
    }

    /// 削除マークされた既存行の論理行番号一覧を返す。
    pub fn deleted_rows(&self) -> &BTreeSet<usize> {
        &self.deleted_rows
    }

    /// 有効な新規追加行の値配列一覧（スロット昇順）を返す。
    pub fn active_inserted_rows(&self) -> Vec<&[CellValue]> {
        self.inserted_rows
            .iter()
            .filter(|s| s.active)
            .map(|s| s.values.as_slice())
            .collect()
    }

    /// 単一セルを更新し、Undo 履歴へ積む。値に変化がなければ何もしない。
    pub fn set_cell(
        &mut self,
        table: &EditableTableData,
        logical_row: usize,
        col: usize,
        new_val: CellValue,
    ) -> bool {
        if col >= self.col_count {
            return false;
        }
        let old_val = self.cell_value(table, logical_row, col).clone();
        if old_val == new_val {
            return false;
        }
        let op = GridEditOp::UpdateCell {
            row: logical_row,
            col,
            old_val,
            new_val,
        };
        self.apply_op(table, &op, true);
        self.undo_stack.push(op);
        self.redo_stack.clear();
        true
    }

    /// 複数の編集操作を 1 ステップの Undo 単位として一括適用する。
    pub fn apply_batch(&mut self, table: &EditableTableData, ops: Vec<GridEditOp>) -> bool {
        if ops.is_empty() {
            return false;
        }
        let batch = GridEditOp::Batch(ops);
        self.apply_op(table, &batch, true);
        self.undo_stack.push(batch);
        self.redo_stack.clear();
        true
    }

    /// 末尾に空（全列 `NULL`）または指定値の新規行を追加し、新しい論理行番号を返す。
    pub fn insert_row(&mut self, table: &EditableTableData, initial_values: Option<Vec<CellValue>>) -> usize {
        let mut values = initial_values.unwrap_or_else(|| vec![CellValue::Null; self.col_count]);
        values.resize(self.col_count, CellValue::Null);
        let row = self.base_row_count + self.inserted_rows.len();
        let op = GridEditOp::InsertRow { row, values };
        self.apply_op(table, &op, true);
        self.undo_stack.push(op);
        self.redo_stack.clear();
        row
    }

    /// 指定した論理行を複製して末尾に新規行として追加し、新しい論理行番号を返す。
    pub fn duplicate_row(&mut self, table: &EditableTableData, source_logical_row: usize) -> usize {
        let values = (0..self.col_count)
            .map(|c| self.cell_value(table, source_logical_row, c).clone())
            .collect();
        self.insert_row(table, Some(values))
    }

    /// 指定した論理行に対して削除マークトグル（既存行）または削除（新規行）を行う。
    pub fn toggle_delete_row(&mut self, table: &EditableTableData, logical_row: usize) -> bool {
        if logical_row < self.base_row_count {
            let deleted_before = self.deleted_rows.contains(&logical_row);
            let op = GridEditOp::ToggleDeleteRow {
                row: logical_row,
                deleted_before,
            };
            self.apply_op(table, &op, true);
            self.undo_stack.push(op);
            self.redo_stack.clear();
            true
        } else {
            let slot_idx = logical_row - self.base_row_count;
            if let Some(slot) = self.inserted_rows.get(slot_idx)
                && slot.active
            {
                let op = GridEditOp::DeactivateInsertedRow { row: logical_row };
                self.apply_op(table, &op, true);
                self.undo_stack.push(op);
                self.redo_stack.clear();
                return true;
            }
            false
        }
    }

    /// 最後の編集操作を 1 ステップ取り消す（Undo）。
    pub fn undo(&mut self, table: &EditableTableData) -> bool {
        let Some(op) = self.undo_stack.pop() else {
            return false;
        };
        self.apply_op(table, &op, false);
        self.redo_stack.push(op);
        true
    }

    /// 取り消した操作を 1 ステップやり直す（Redo）。
    pub fn redo(&mut self, table: &EditableTableData) -> bool {
        let Some(op) = self.redo_stack.pop() else {
            return false;
        };
        self.apply_op(table, &op, true);
        self.undo_stack.push(op);
        true
    }

    /// すべての未反映変更と Undo/Redo 履歴を破棄する。
    pub fn clear(&mut self) {
        self.cell_edits.clear();
        self.deleted_rows.clear();
        self.inserted_rows.clear();
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// 操作を正方向（`forward = true`）または逆方向（`forward = false`）に適用する内部関数。
    fn apply_op(&mut self, table: &EditableTableData, op: &GridEditOp, forward: bool) {
        match op {
            GridEditOp::UpdateCell {
                row,
                col,
                old_val,
                new_val,
            } => {
                let target = if forward { new_val } else { old_val };
                if *row < self.base_row_count {
                    let orig = table.result.get(*row, *col).unwrap_or(&CellValue::Null);
                    if target == orig {
                        self.cell_edits.remove(&(*row, *col));
                    } else {
                        self.cell_edits.insert((*row, *col), target.clone());
                    }
                } else {
                    let slot_idx = *row - self.base_row_count;
                    if let Some(slot) = self.inserted_rows.get_mut(slot_idx)
                        && *col < slot.values.len()
                    {
                        slot.values[*col] = target.clone();
                    }
                }
            }
            GridEditOp::InsertRow { row, values } => {
                let slot_idx = *row - self.base_row_count;
                if forward {
                    if slot_idx < self.inserted_rows.len() {
                        self.inserted_rows[slot_idx].active = true;
                        self.inserted_rows[slot_idx].values = values.clone();
                    } else {
                        while self.inserted_rows.len() < slot_idx {
                            self.inserted_rows.push(InsertedRowSlot {
                                values: vec![CellValue::Null; self.col_count],
                                active: false,
                            });
                        }
                        self.inserted_rows.push(InsertedRowSlot {
                            values: values.clone(),
                            active: true,
                        });
                    }
                } else if let Some(slot) = self.inserted_rows.get_mut(slot_idx) {
                    slot.active = false;
                }
            }
            GridEditOp::DeactivateInsertedRow { row } => {
                let slot_idx = *row - self.base_row_count;
                if let Some(slot) = self.inserted_rows.get_mut(slot_idx) {
                    slot.active = !forward;
                }
            }
            GridEditOp::ToggleDeleteRow {
                row,
                deleted_before,
            } => {
                let should_delete = if forward { !deleted_before } else { *deleted_before };
                if should_delete {
                    self.deleted_rows.insert(*row);
                } else {
                    self.deleted_rows.remove(row);
                }
            }
            GridEditOp::Batch(ops) => {
                if forward {
                    for sub in ops {
                        self.apply_op(table, sub, true);
                    }
                } else {
                    for sub in ops.iter().rev() {
                        self.apply_op(table, sub, false);
                    }
                }
            }
        }
    }
}

/// グリッドの選択範囲に対するリアルタイム集計結果。
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionStats {
    /// 選択されたセル総数。
    pub cell_count: usize,
    /// NULL 以外のセル数。
    pub non_null_count: usize,
    /// 数値として解釈できたセル数。
    pub numeric_count: usize,
    /// 数値セルの合計値。
    pub sum: f64,
    /// 数値セルの平均値（数値セルがある場合のみ `Some`）。
    pub avg: Option<f64>,
    /// 数値セルの最小値（数値セルがある場合のみ `Some`）。
    pub min: Option<f64>,
    /// 数値セルの最大値（数値セルがある場合のみ `Some`）。
    pub max: Option<f64>,
}

/// セル配列（行×列のイテレータ等）から選択範囲の統計情報を計算する。
pub fn compute_selection_stats<'a, I>(cells: I) -> SelectionStats
where
    I: IntoIterator<Item = &'a CellValue>,
{
    let mut cell_count = 0usize;
    let mut non_null_count = 0usize;
    let mut numeric_count = 0usize;
    let mut sum = 0.0f64;
    let mut min: Option<f64> = None;
    let mut max: Option<f64> = None;

    for cell in cells {
        cell_count += 1;
        if !matches!(cell, CellValue::Null) {
            non_null_count += 1;
        }
        if let Some(val) = cell_as_f64(cell) {
            numeric_count += 1;
            sum += val;
            min = Some(match min {
                Some(cur) => cur.min(val),
                None => val,
            });
            max = Some(match max {
                Some(cur) => cur.max(val),
                None => val,
            });
        }
    }

    let avg = if numeric_count > 0 {
        Some(sum / numeric_count as f64)
    } else {
        None
    };

    SelectionStats {
        cell_count,
        non_null_count,
        numeric_count,
        sum,
        avg,
        min,
        max,
    }
}

/// `CellValue` を統計計算用の有限な `f64` に変換する。
fn cell_as_f64(cell: &CellValue) -> Option<f64> {
    match cell {
        CellValue::Int(n) => Some(*n as f64),
        CellValue::Float(f) if f.is_finite() => Some(*f),
        CellValue::Decimal(s) => s.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
        _ => None,
    }
}

/// カラム型文字列が数値型かどうかを判定する。
fn is_numeric_data_type(data_type: &str) -> bool {
    let upper = data_type.to_ascii_uppercase();
    upper.contains("INT")
        || upper.contains("NUM")
        || upper.contains("DEC")
        || upper.contains("REAL")
        || upper.contains("FLOAT")
        || upper.contains("DOUBLE")
}

/// ユーザーがインラインエディタやペーストで入力した文字列を、列定義に応じて `CellValue` に変換する。
pub fn parse_cell_input(raw: &str, col_meta: &ColumnMeta, treat_empty_as_null: bool) -> CellValue {
    if treat_empty_as_null && raw.is_empty() {
        return CellValue::Null;
    }
    if is_numeric_data_type(&col_meta.data_type) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return CellValue::Null;
        }
        if let Ok(n) = trimmed.parse::<i64>() {
            return CellValue::Int(n);
        }
        if let Ok(f) = trimmed.parse::<f64>()
            && f.is_finite()
        {
            return CellValue::Decimal(CompactString::new(trimmed));
        }
    }
    CellValue::Text(CompactString::new(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_table() -> EditableTableData {
        let cols = vec![
            ColumnMeta {
                name: "ID".into(),
                data_type: "INTEGER".into(),
                nullable: false,
            },
            ColumnMeta {
                name: "NAME".into(),
                data_type: "TEXT".into(),
                nullable: true,
            },
        ];
        let mut rs = ResultSet::with_columns(cols.clone());
        rs.push_row(vec![CellValue::Int(1), CellValue::Text("Alice".into())]);
        rs.push_row(vec![CellValue::Int(2), CellValue::Text("Bob".into())]);
        EditableTableData {
            dialect: SqlDialect::Sqlite,
            owner: "main".to_string(),
            table_name: "USERS".to_string(),
            where_clause: String::new(),
            order_by: String::new(),
            columns: cols,
            row_keys: vec![
                Some(RowKey::RowId("1".to_string())),
                Some(RowKey::RowId("2".to_string())),
            ],
            result: rs,
        }
    }

    #[test]
    fn grid_patch_undo_redo_and_revert_to_original() {
        let table = sample_table();
        let mut patch = GridPatch::new(2, 2);
        assert!(!patch.has_changes());

        assert!(patch.set_cell(&table, 0, 1, CellValue::Text("Alice2".into())));
        assert!(patch.has_changes());
        assert_eq!(patch.modified_existing_row_count(), 1);
        assert_eq!(
            patch.cell_value(&table, 0, 1),
            &CellValue::Text("Alice2".into())
        );

        // 元の値に戻すと変更差分が消える
        assert!(patch.set_cell(&table, 0, 1, CellValue::Text("Alice".into())));
        assert!(!patch.has_changes());

        // Undo で "Alice2" に戻り、さらに Undo で元に戻る
        assert!(patch.undo(&table));
        assert_eq!(
            patch.cell_value(&table, 0, 1),
            &CellValue::Text("Alice2".into())
        );
        assert!(patch.undo(&table));
        assert!(!patch.has_changes());

        // Redo で "Alice2"
        assert!(patch.redo(&table));
        assert_eq!(
            patch.cell_value(&table, 0, 1),
            &CellValue::Text("Alice2".into())
        );
    }

    #[test]
    fn grid_patch_insert_duplicate_delete_and_undo() {
        let table = sample_table();
        let mut patch = GridPatch::new(2, 2);

        let new_row = patch.duplicate_row(&table, 1);
        assert_eq!(new_row, 2);
        assert_eq!(patch.inserted_row_count(), 1);
        assert_eq!(patch.cell_value(&table, 2, 0), &CellValue::Int(2));
        assert_eq!(
            patch.cell_value(&table, 2, 1),
            &CellValue::Text("Bob".into())
        );

        // 新規行をさらに編集してから削除 -> Undo で復帰すること
        assert!(patch.set_cell(&table, new_row, 1, CellValue::Text("BobCopy".into())));
        assert!(patch.toggle_delete_row(&table, new_row));
        assert_eq!(patch.inserted_row_count(), 0);
        assert_eq!(patch.display_rows().len(), 2);

        assert!(patch.undo(&table));
        assert_eq!(patch.inserted_row_count(), 1);
        assert_eq!(
            patch.cell_value(&table, new_row, 1),
            &CellValue::Text("BobCopy".into())
        );

        // 既存行の削除マーク
        assert!(patch.toggle_delete_row(&table, 0));
        assert!(patch.is_row_deleted(0));
        assert_eq!(patch.deleted_row_count(), 1);
        assert!(patch.undo(&table));
        assert!(!patch.is_row_deleted(0));
    }

    #[test]
    fn selection_stats_calculates_numeric_aggregates() {
        let cells = [
            CellValue::Int(10),
            CellValue::Float(20.5),
            CellValue::Decimal("9.5".into()),
            CellValue::Text("abc".into()),
            CellValue::Null,
        ];
        let stats = compute_selection_stats(cells.iter());
        assert_eq!(stats.cell_count, 5);
        assert_eq!(stats.non_null_count, 4);
        assert_eq!(stats.numeric_count, 3);
        assert!((stats.sum - 40.0).abs() < 1e-9);
        assert!((stats.avg.unwrap() - (40.0 / 3.0)).abs() < 1e-9);
        assert_eq!(stats.min, Some(9.5));
        assert_eq!(stats.max, Some(20.5));
    }
}
