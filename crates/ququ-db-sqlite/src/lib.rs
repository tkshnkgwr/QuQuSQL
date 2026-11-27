//! `rusqlite`（bundled）を用いた SQLite データベースドライバ実装。
//!
//! デフォルトで自動コミットを無効化するため、DML 実行前に明示的な
//! `BEGIN TRANSACTION` を開始し、ユーザー操作でのみ確定・取消します。

#![warn(missing_docs)]

use anyhow::{Context, Result};
use compact_str::CompactString;
use ququ_core::{
    CellValue, ColumnDetail, ColumnMeta, EditableTableData, ObjectType, ParameterizedDml,
    ResultSet, RowKey, SchemaObject, SqlDialect, quote_identifier,
};
use ququ_db::DbSession;
use rusqlite::{Connection, types::ValueRef};
use std::time::Instant;

/// SQLite データベースセッション。
pub struct SqliteSession {
    conn: Connection,
    target_label: String,
    uncommitted_ops: usize,
}

impl SqliteSession {
    /// 指定パスの SQLite ファイル（空文字または `:memory:` ならインメモリ）を開く。
    pub fn open(file_path: &str) -> Result<Self> {
        let trimmed = file_path.trim();
        let conn = if trimmed.is_empty() || trimmed == ":memory:" {
            Connection::open_in_memory().context("インメモリ SQLite の起動に失敗しました")?
        } else {
            Connection::open(trimmed)
                .with_context(|| format!("SQLite ファイルを開けません: {trimmed}"))?
        };
        let label = if trimmed.is_empty() {
            ":memory:".to_string()
        } else {
            trimmed.to_string()
        };
        Ok(Self {
            conn,
            target_label: label,
            uncommitted_ops: 0,
        })
    }

    /// 自動コミット状態のままであれば、手動トランザクションを開始して不意な確定を防ぐ。
    fn ensure_manual_tx(&mut self) -> Result<()> {
        if self.conn.is_autocommit() {
            self.conn
                .execute_batch("BEGIN TRANSACTION;")
                .context("手動トランザクション開始に失敗しました")?;
        }
        Ok(())
    }

    /// 指定されたオフセットと上限件数で SELECT クエリを実行し、結果セットを構築する。
    fn run_query_window(&mut self, sql: &str, offset: usize, limit: usize) -> Result<ResultSet> {
        let start = Instant::now();
        let mut stmt = self.conn.prepare(sql)?;
        let col_count = stmt.column_count();
        let columns: Vec<ColumnMeta> = stmt
            .column_names()
            .into_iter()
            .map(|name| ColumnMeta {
                name: CompactString::from(name),
                data_type: CompactString::new(""),
                nullable: true,
            })
            .collect();

        let mut rs = ResultSet::with_columns(columns);
        rs.source_sql = Some(sql.to_string());

        let mut rows = stmt.query([])?;
        // 追加フェッチ時は指定オフセットまで読み飛ばす
        let mut skipped = 0usize;
        while skipped < offset {
            if rows.next()?.is_none() {
                rs.elapsed_ms = start.elapsed().as_millis() as u64;
                return Ok(rs);
            }
            skipped += 1;
        }

        let max_rows = if limit == 0 { usize::MAX } else { limit };
        while let Some(row) = rows.next()? {
            if rs.row_count >= max_rows {
                // 上限件数に達したため、さらに後続行が存在することを記録して一旦停止する
                rs.has_more = true;
                break;
            }
            let mut vals = Vec::with_capacity(col_count);
            for col_idx in 0..col_count {
                let cell = match row.get_ref(col_idx)? {
                    ValueRef::Null => CellValue::Null,
                    ValueRef::Integer(i) => CellValue::Int(i),
                    ValueRef::Real(f) => CellValue::Float(f),
                    ValueRef::Text(bytes) => {
                        let s = String::from_utf8_lossy(bytes);
                        CellValue::Text(CompactString::from(s.as_ref()))
                    }
                    ValueRef::Blob(bytes) => CellValue::Bytes(bytes.into()),
                };
                vals.push(cell);
            }
            rs.push_row(vals);
        }

        rs.elapsed_ms = start.elapsed().as_millis() as u64;
        Ok(rs)
    }
}

impl DbSession for SqliteSession {
    fn execute_query(&mut self, sql: &str, fetch_limit: usize) -> Result<ResultSet> {
        self.run_query_window(sql, 0, fetch_limit)
    }

    fn fetch_more(&mut self, sql: &str, offset: usize, limit: usize) -> Result<ResultSet> {
        self.run_query_window(sql, offset, limit)
    }

    fn execute_dml(&mut self, sql: &str) -> Result<u64> {
        self.ensure_manual_tx()?;
        let affected = self.conn.execute(sql, [])? as u64;
        self.uncommitted_ops += 1;
        Ok(affected)
    }

    fn execute_ddl(&mut self, sql: &str) -> Result<()> {
        self.conn.execute_batch(sql)?;
        Ok(())
    }

    fn commit(&mut self) -> Result<()> {
        if !self.conn.is_autocommit() {
            self.conn.execute_batch("COMMIT;")?;
        }
        self.uncommitted_ops = 0;
        Ok(())
    }

    fn rollback(&mut self) -> Result<()> {
        if !self.conn.is_autocommit() {
            self.conn.execute_batch("ROLLBACK;")?;
        }
        self.uncommitted_ops = 0;
        Ok(())
    }

    fn has_uncommitted_changes(&self) -> bool {
        !self.conn.is_autocommit() && self.uncommitted_ops > 0
    }

    fn uncommitted_count(&self) -> usize {
        if self.conn.is_autocommit() {
            0
        } else {
            self.uncommitted_ops
        }
    }

    fn list_owners(&mut self) -> Result<Vec<String>> {
        // SQLite の標準メインデータベース名
        Ok(vec!["main".to_string()])
    }

    fn list_objects(&mut self, owner: &str, object_type: &ObjectType) -> Result<Vec<SchemaObject>> {
        let type_filter = match object_type {
            ObjectType::Table => "table",
            ObjectType::View => "view",
            ObjectType::Index => "index",
            ObjectType::Trigger => "trigger",
            _ => return Ok(Vec::new()),
        };
        let mut stmt = self.conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = ?1 AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let rows = stmt.query_map([type_filter], |row| row.get::<_, String>(0))?;
        let mut list = Vec::new();
        for name_res in rows {
            let name = name_res?;
            list.push(SchemaObject {
                owner: owner.to_string(),
                name,
                object_type: object_type.clone(),
                comments: String::new(),
            });
        }
        Ok(list)
    }

    fn list_columns(&mut self, _owner: &str, table_name: &str) -> Result<Vec<ColumnDetail>> {
        let escaped = table_name.replace('"', "\"\"");
        let sql = format!("PRAGMA table_info(\"{escaped}\")");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            let name: String = row.get(1)?;
            let raw_type: String = row.get(2)?;
            let (data_type, length) = split_type_and_length(&raw_type);
            let notnull: i64 = row.get(3)?;
            let pk: i64 = row.get(5)?;
            Ok(ColumnDetail {
                name,
                data_type,
                length,
                nullable: notnull == 0,
                is_pk: pk > 0,
                comments: String::new(),
            })
        })?;

        let mut cols = Vec::new();
        for col in rows {
            cols.push(col?);
        }
        Ok(cols)
    }

    fn get_ddl(&mut self, _owner: &str, _object_type: &ObjectType, name: &str) -> Result<String> {
        let sql: Option<String> = self
            .conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = ?1 LIMIT 1",
                [name],
                |row| row.get(0),
            )
            .ok();
        Ok(sql.unwrap_or_else(|| format!("-- DDL が見つかりません: {name}")))
    }

    fn load_table_for_edit(
        &mut self,
        owner: &str,
        table_name: &str,
        where_clause: &str,
        order_by: &str,
        fetch_limit: usize,
    ) -> Result<EditableTableData> {
        let start = Instant::now();
        let tbl_ident = if owner.trim().is_empty() {
            quote_identifier(table_name, SqlDialect::Sqlite)
        } else {
            format!(
                "{}.{}",
                quote_identifier(owner, SqlDialect::Sqlite),
                quote_identifier(table_name, SqlDialect::Sqlite)
            )
        };

        let mut suffix = String::new();
        if !where_clause.trim().is_empty() {
            suffix.push_str(&format!(" WHERE {}", where_clause.trim()));
        }
        if !order_by.trim().is_empty() {
            suffix.push_str(&format!(" ORDER BY {}", order_by.trim()));
        }

        let rowid_sql = format!("SELECT t.rowid AS _ququ_rowid, t.* FROM {tbl_ident} t{suffix}");
        let fallback_sql = format!("SELECT NULL AS _ququ_rowid, t.* FROM {tbl_ident} t{suffix}");

        let use_rowid = self.conn.prepare(&rowid_sql).is_ok();
        let exec_sql = if use_rowid { rowid_sql } else { fallback_sql };

        let col_details = self.list_columns(owner, table_name).unwrap_or_default();
        let mut stmt = self.conn.prepare(&exec_sql)?;
        let total_cols = stmt.column_count();
        let mut columns = Vec::with_capacity(total_cols.saturating_sub(1));
        for col_name in stmt.column_names().into_iter().skip(1) {
            let matched = col_details
                .iter()
                .find(|d| d.name.eq_ignore_ascii_case(col_name));
            columns.push(ColumnMeta {
                name: CompactString::from(col_name),
                data_type: CompactString::from(
                    matched.map(|m| m.data_type.as_str()).unwrap_or(""),
                ),
                nullable: matched.is_none_or(|m| m.nullable),
            });
        }

        let mut rs = ResultSet::with_columns(columns.clone());
        rs.source_sql = Some(format!("SELECT * FROM {tbl_ident}{suffix}"));
        let mut row_keys = Vec::new();

        let mut rows = stmt.query([])?;
        let max_rows = if fetch_limit == 0 {
            usize::MAX
        } else {
            fetch_limit
        };
        while let Some(row) = rows.next()? {
            if rs.row_count >= max_rows {
                rs.has_more = true;
                break;
            }
            let key = match row.get_ref(0)? {
                ValueRef::Integer(id) => Some(RowKey::RowId(id.to_string())),
                ValueRef::Text(bytes) => {
                    let s = String::from_utf8_lossy(bytes);
                    Some(RowKey::RowId(s.into_owned()))
                }
                _ => None,
            };
            row_keys.push(key);

            let mut vals = Vec::with_capacity(total_cols.saturating_sub(1));
            for col_idx in 1..total_cols {
                let cell = match row.get_ref(col_idx)? {
                    ValueRef::Null => CellValue::Null,
                    ValueRef::Integer(i) => CellValue::Int(i),
                    ValueRef::Real(f) => CellValue::Float(f),
                    ValueRef::Text(bytes) => {
                        let s = String::from_utf8_lossy(bytes);
                        CellValue::Text(CompactString::from(s.as_ref()))
                    }
                    ValueRef::Blob(bytes) => CellValue::Bytes(bytes.into()),
                };
                vals.push(cell);
            }
            rs.push_row(vals);
        }

        rs.elapsed_ms = start.elapsed().as_millis() as u64;
        Ok(EditableTableData {
            dialect: SqlDialect::Sqlite,
            owner: owner.to_string(),
            table_name: table_name.to_string(),
            where_clause: where_clause.to_string(),
            order_by: order_by.to_string(),
            columns,
            row_keys,
            result: rs,
        })
    }

    fn apply_dml_batch(&mut self, dmls: &[ParameterizedDml]) -> Result<usize> {
        if dmls.is_empty() {
            return Ok(0);
        }
        self.ensure_manual_tx()?;
        self.conn
            .execute_batch("SAVEPOINT ququ_grid_apply;")
            .context("SAVEPOINT 開始に失敗しました")?;

        let mut total_affected = 0usize;
        for dml in dmls {
            let params: Vec<rusqlite::types::Value> =
                dml.params.iter().map(cell_to_sqlite_value).collect();
            let exec_res = self
                .conn
                .execute(&dml.sql, rusqlite::params_from_iter(params.iter()));
            let affected = match exec_res {
                Ok(cnt) => cnt,
                Err(e) => {
                    let _ = self.conn.execute_batch(
                        "ROLLBACK TO SAVEPOINT ququ_grid_apply; RELEASE SAVEPOINT ququ_grid_apply;",
                    );
                    return Err(e).with_context(|| {
                        format!(
                            "DML 実行に失敗し SAVEPOINT へロールバックしました: {}",
                            dml.preview_sql
                        )
                    });
                }
            };

            if dml.expect_one_row && affected != 1 {
                let _ = self.conn.execute_batch(
                    "ROLLBACK TO SAVEPOINT ququ_grid_apply; RELEASE SAVEPOINT ququ_grid_apply;",
                );
                anyhow::bail!(
                    "対象行の整合性エラー: 1 行の予定が {affected} 行に影響したため SAVEPOINT へロールバックしました: {}",
                    dml.preview_sql
                );
            }
            total_affected += affected;
        }

        self.conn
            .execute_batch("RELEASE SAVEPOINT ququ_grid_apply;")
            .context("SAVEPOINT 確定解放に失敗しました")?;
        // 自動 COMMIT せず未コミット状態のまま保持する
        self.uncommitted_ops += dmls.len();
        Ok(total_affected)
    }

    fn db_info(&self) -> String {
        format!("SQLite ({})", self.target_label)
    }
}

/// `CellValue` を SQLite バインド用の `rusqlite::types::Value` へ変換する。
fn cell_to_sqlite_value(cell: &CellValue) -> rusqlite::types::Value {
    use rusqlite::types::Value;
    match cell {
        CellValue::Null => Value::Null,
        CellValue::Bool(b) => Value::Integer(if *b { 1 } else { 0 }),
        CellValue::Int(i) => Value::Integer(*i),
        CellValue::Float(f) => Value::Real(*f),
        CellValue::Decimal(s) => {
            if let Ok(i) = s.trim().parse::<i64>() {
                Value::Integer(i)
            } else if let Ok(f) = s.trim().parse::<f64>() {
                Value::Real(f)
            } else {
                Value::Text(s.to_string())
            }
        }
        CellValue::Text(s) | CellValue::DateTime(s) => Value::Text(s.to_string()),
        CellValue::Bytes(b) => Value::Blob(b.to_vec()),
    }
}

/// SQLite の型定義文字列（例: `char(20)`, `number(10, 2)`）から型名と長さ部分を分離する。
fn split_type_and_length(raw: &str) -> (String, String) {
    let trimmed = raw.trim();
    if let Some(open_idx) = trimmed.find('(')
        && let Some(rel_close) = trimmed[open_idx + 1..].rfind(')')
    {
        let type_name = trimmed[..open_idx].trim().to_string();
        let inner = trimmed[open_idx + 1..open_idx + 1 + rel_close]
            .split(',')
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(",");
        return (type_name, inner);
    }
    (trimmed.to_string(), String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_sqlite_type_and_length() {
        assert_eq!(
            split_type_and_length("char(20)"),
            ("char".to_string(), "20".to_string())
        );
        assert_eq!(
            split_type_and_length("number(10, 2)"),
            ("number".to_string(), "10,2".to_string())
        );
        assert_eq!(
            split_type_and_length("number"),
            ("number".to_string(), String::new())
        );

        let mut sess = SqliteSession::open(":memory:").unwrap();
        sess.execute_ddl("CREATE TABLE sample (id number(10), pass char(20), note text)")
            .unwrap();
        let cols = sess.list_columns("main", "sample").unwrap();
        assert!(cols[0].data_type.eq_ignore_ascii_case("number"));
        assert_eq!(cols[0].length, "10");
        assert!(cols[1].data_type.eq_ignore_ascii_case("char"));
        assert_eq!(cols[1].length, "20");
        assert!(cols[2].data_type.eq_ignore_ascii_case("text"));
        assert_eq!(cols[2].length, "");
    }

    #[test]
    fn manual_transaction_prevents_accidental_commit() {
        let mut sess = SqliteSession::open(":memory:").unwrap();
        sess.execute_ddl("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();

        let affected = sess
            .execute_dml("INSERT INTO items (id, name) VALUES (1, 'temp')")
            .unwrap();
        assert_eq!(affected, 1);
        assert!(sess.has_uncommitted_changes());
        assert_eq!(sess.uncommitted_count(), 1);

        // ロールバックで行が消えること
        sess.rollback().unwrap();
        assert!(!sess.has_uncommitted_changes());
        let rs = sess.execute_query("SELECT id, name FROM items", 1000).unwrap();
        assert_eq!(rs.row_count, 0);

        // コミットで永続化されること
        sess.execute_dml("INSERT INTO items (id, name) VALUES (2, 'saved')")
            .unwrap();
        sess.commit().unwrap();
        assert!(!sess.has_uncommitted_changes());
        let rs2 = sess.execute_query("SELECT id, name FROM items", 1000).unwrap();
        assert_eq!(rs2.row_count, 1);
        assert_eq!(rs2.get(0, 0), Some(&CellValue::Int(2)));
    }

    #[test]
    fn respects_fetch_limit_and_metadata() {
        let mut sess = SqliteSession::open(":memory:").unwrap();
        sess.execute_ddl("CREATE TABLE nums (n INTEGER PRIMARY KEY, label TEXT NOT NULL)")
            .unwrap();
        for i in 1..=15 {
            sess.execute_dml(&format!("INSERT INTO nums VALUES ({i}, 'row{i}')"))
                .unwrap();
        }
        sess.commit().unwrap();

        let page1 = sess
            .execute_query("SELECT n, label FROM nums ORDER BY n", 10)
            .unwrap();
        assert_eq!(page1.row_count, 10);
        assert!(page1.has_more);

        let page2 = sess
            .fetch_more("SELECT n, label FROM nums ORDER BY n", 10, 10)
            .unwrap();
        assert_eq!(page2.row_count, 5);
        assert!(!page2.has_more);

        let objs = sess.list_objects("main", &ObjectType::Table).unwrap();
        assert_eq!(objs.len(), 1);
        assert_eq!(objs[0].name, "nums");

        let cols = sess.list_columns("main", "nums").unwrap();
        assert_eq!(cols.len(), 2);
        assert!(cols[0].is_pk);
        assert!(!cols[1].nullable);

        let ddl = sess.get_ddl("main", &ObjectType::Table, "nums").unwrap();
        assert!(ddl.contains("CREATE TABLE nums"));
    }

    #[test]
    fn roundtrips_all_value_types_and_rolls_back_update_delete() {
        let mut sess = SqliteSession::open(":memory:").unwrap();
        sess.execute_ddl("CREATE TABLE mix (id INTEGER PRIMARY KEY, f REAL, t TEXT, b BLOB)")
            .unwrap();
        sess.execute_dml("INSERT INTO mix VALUES (1, 3.5, 'こんにちは', X'CAFEBABE')")
            .unwrap();
        sess.execute_dml("INSERT INTO mix VALUES (2, NULL, NULL, NULL)")
            .unwrap();
        sess.commit().unwrap();

        let rs = sess
            .execute_query("SELECT id, f, t, b FROM mix ORDER BY id", 100)
            .unwrap();
        assert_eq!(rs.get(0, 0), Some(&CellValue::Int(1)));
        assert_eq!(rs.get(0, 1), Some(&CellValue::Float(3.5)));
        assert_eq!(
            rs.get(0, 2),
            Some(&CellValue::Text(CompactString::from("こんにちは")))
        );
        assert_eq!(
            rs.get(0, 3),
            Some(&CellValue::Bytes(vec![0xCA, 0xFE, 0xBA, 0xBE].into()))
        );
        assert_eq!(rs.get(1, 2), Some(&CellValue::Null));

        // UPDATE と DELETE を実行した後に ROLLBACK で元に戻ること
        let updated = sess
            .execute_dml("UPDATE mix SET t = 'changed' WHERE id = 1")
            .unwrap();
        assert_eq!(updated, 1);
        let deleted = sess.execute_dml("DELETE FROM mix WHERE id = 2").unwrap();
        assert_eq!(deleted, 1);
        sess.rollback().unwrap();

        let after_rb = sess
            .execute_query("SELECT id, t FROM mix ORDER BY id", 100)
            .unwrap();
        assert_eq!(after_rb.row_count, 2);
        assert_eq!(
            after_rb.get(0, 1),
            Some(&CellValue::Text(CompactString::from("こんにちは")))
        );
    }

    #[test]
    fn grid_edit_load_apply_and_savepoint_rollback() {
        let mut sess = SqliteSession::open(":memory:").unwrap();
        sess.execute_ddl("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, price NUMBER(10, 2))")
            .unwrap();
        sess.execute_dml("INSERT INTO items VALUES (1, 'Apple', 100)")
            .unwrap();
        sess.execute_dml("INSERT INTO items VALUES (2, 'Banana', 200)")
            .unwrap();
        sess.commit().unwrap();

        let table = sess
            .load_table_for_edit("main", "items", "price >= 100", "id DESC", 50)
            .unwrap();
        assert_eq!(table.result.row_count, 2);
        assert_eq!(table.result.get(0, 0), Some(&CellValue::Int(2)));
        assert!(table.all_rows_editable());

        let mut patch = ququ_core::GridPatch::new(table.result.row_count, table.columns.len());
        patch.set_cell(&table, 0, 1, CellValue::Text("BananaPro".into()));
        let new_row = patch.insert_row(&table, None);
        patch.set_cell(&table, new_row, 0, CellValue::Int(3));
        patch.set_cell(&table, new_row, 1, CellValue::Text("Cherry".into()));

        let dmls = ququ_sql::generate_grid_dml(&table, &patch).unwrap();
        let applied = sess.apply_dml_batch(&dmls).unwrap();
        assert_eq!(applied, 2);
        assert!(sess.has_uncommitted_changes());

        // 自動 COMMIT されないため、ROLLBACK するとすべて元に戻る
        sess.rollback().unwrap();
        assert!(!sess.has_uncommitted_changes());
        let check = sess
            .execute_query("SELECT name FROM items WHERE id = 2", 10)
            .unwrap();
        assert_eq!(check.get(0, 0), Some(&CellValue::Text("Banana".into())));

        // 存在しない rowid を含むバッチは SAVEPOINT で全体ロールバックされる
        let bad_batch = vec![
            ParameterizedDml {
                sql: "UPDATE main.items SET name = ?1 WHERE id = ?2".to_string(),
                params: vec![CellValue::Text("Tmp".into()), CellValue::Int(1)],
                preview_sql: String::new(),
                expect_one_row: true,
            },
            ParameterizedDml {
                sql: "DELETE FROM main.items WHERE rowid = ?1".to_string(),
                params: vec![CellValue::Int(999999)],
                preview_sql: String::new(),
                expect_one_row: true,
            },
        ];
        assert!(sess.apply_dml_batch(&bad_batch).is_err());
        let intact = sess
            .execute_query("SELECT name FROM items WHERE id = 1", 10)
            .unwrap();
        assert_eq!(intact.get(0, 0), Some(&CellValue::Text("Apple".into())));
    }
}
