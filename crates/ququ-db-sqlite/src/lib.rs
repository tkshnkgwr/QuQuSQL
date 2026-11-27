//! `rusqlite`（bundled）を用いた SQLite データベースドライバ実装。
//!
//! デフォルトで自動コミットを無効化するため、DML 実行前に明示的な
//! `BEGIN TRANSACTION` を開始し、ユーザー操作でのみ確定・取消します。

#![warn(missing_docs)]

use anyhow::{Context, Result};
use compact_str::CompactString;
use ququ_core::{CellValue, ColumnDetail, ColumnMeta, ObjectType, ResultSet, SchemaObject};
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
            let data_type: String = row.get(2)?;
            let notnull: i64 = row.get(3)?;
            let pk: i64 = row.get(5)?;
            Ok(ColumnDetail {
                name,
                data_type,
                length: String::new(),
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

    fn db_info(&self) -> String {
        format!("SQLite ({})", self.target_label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
