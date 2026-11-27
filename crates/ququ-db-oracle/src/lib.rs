use anyhow::{Context, Result, anyhow};
use compact_str::CompactString;
use oracle_rs::{ColumnInfo, Config, Connection, Row, Value};
use ququ_core::{
    CellValue, ColumnDetail, ColumnMeta, ConnectCredentials, ConnectionProfile, ObjectType,
    OracleConnectMode, ResultSet, SchemaObject, find_tnsnames_path, parse_tnsnames,
};
use ququ_db::DbSession;
use std::fmt::Write as _;
use std::fs;
use std::time::Instant;
use tokio::runtime::Runtime;

pub fn build_oracle_config(
    profile: &ConnectionProfile,
    credentials: &ConnectCredentials,
    tns_admin_override: Option<&str>,
) -> Result<Config> {
    let username = profile.username.trim();
    if username.is_empty() {
        return Err(anyhow!("Oracle ユーザー名が入力されていません。"));
    }
    let password = credentials.password.as_deref().unwrap_or("");

    match profile.connect_mode {
        OracleConnectMode::Direct => {
            if profile.host.trim().is_empty() {
                return Err(anyhow!("ホスト名が入力されていません。"));
            }
            if profile.service_name.trim().is_empty() {
                return Err(anyhow!("サービス名が入力されていません。"));
            }
            Ok(Config::new(
                profile.host.trim(),
                profile.port,
                profile.service_name.trim(),
                username,
                password,
            ))
        }
        OracleConnectMode::Tns => {
            let alias = profile.tns_name.trim();
            if alias.is_empty() {
                return Err(anyhow!("TNS 名が入力されていません。"));
            }
            let tns_path = find_tnsnames_path(tns_admin_override).ok_or_else(|| {
                anyhow!("tnsnames.ora が見つかりません。設定画面または環境変数 TNS_ADMIN を確認してください。")
            })?;
            let content = fs::read_to_string(&tns_path)
                .with_context(|| format!("tnsnames.ora の読み込みに失敗しました: {}", tns_path.display()))?;
            let entries = parse_tnsnames(&content);
            let entry = entries
                .into_iter()
                .find(|e| e.alias.eq_ignore_ascii_case(alias))
                .ok_or_else(|| anyhow!("tnsnames.ora 内に接続先 '{alias}' が見つかりません。"))?;

            if let Some(service_name) = entry.service_name {
                Ok(Config::new(
                    entry.host,
                    entry.port,
                    service_name,
                    username,
                    password,
                ))
            } else if let Some(sid) = entry.sid {
                Ok(Config::with_sid(
                    entry.host,
                    entry.port,
                    sid,
                    username,
                    password,
                ))
            } else {
                Err(anyhow!(
                    "TNS エントリ '{alias}' に SERVICE_NAME または SID が定義されていません。"
                ))
            }
        }
    }
}

struct PendingCursor {
    sql: String,
    cursor_id: u16,
    columns: Vec<ColumnInfo>,
}

pub struct OracleSession {
    rt: Runtime,
    conn: Connection,
    target_label: String,
    uncommitted_ops: usize,
    pending_cursor: Option<PendingCursor>,
}

impl OracleSession {
    pub fn connect(
        profile: &ConnectionProfile,
        credentials: &ConnectCredentials,
        tns_admin_override: Option<&str>,
    ) -> Result<Self> {
        let config = build_oracle_config(profile, credentials, tns_admin_override)?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .context("Tokio ランタイムの初期化に失敗しました")?;

        let conn = rt
            .block_on(Connection::connect_with_config(config))
            .context("Oracle データベースへの接続に失敗しました")?;

        Ok(Self {
            rt,
            conn,
            target_label: profile.display_target(),
            uncommitted_ops: 0,
            pending_cursor: None,
        })
    }

    fn convert_columns(oracle_cols: &[ColumnInfo]) -> Vec<ColumnMeta> {
        oracle_cols
            .iter()
            .map(|c| ColumnMeta {
                name: CompactString::from(c.name.as_str()),
                data_type: CompactString::from(format!("{:?}", c.oracle_type)),
                nullable: c.nullable,
            })
            .collect()
    }

    fn push_rows(rs: &mut ResultSet, rows: Vec<Row>) {
        for row in rows {
            let cells = row.into_values().into_iter().map(convert_value).collect();
            rs.push_row(cells);
        }
    }
}

impl DbSession for OracleSession {
    fn execute_query(&mut self, sql: &str, fetch_limit: usize) -> Result<ResultSet> {
        self.pending_cursor = None;
        let start = Instant::now();
        let initial = self.rt.block_on(self.conn.query(sql, &[]))?;

        let columns_info = initial.columns.clone();
        let mut cursor_id = initial.cursor_id;
        let mut has_more = initial.has_more_rows;

        let mut rs = ResultSet::with_columns(Self::convert_columns(&columns_info));
        rs.source_sql = Some(sql.to_string());
        Self::push_rows(&mut rs, initial.rows);

        let limit = if fetch_limit == 0 {
            usize::MAX
        } else {
            fetch_limit
        };

        while has_more && rs.row_count < limit {
            let remaining = (limit - rs.row_count).min(500) as u32;
            let next = self
                .rt
                .block_on(self.conn.fetch_more(cursor_id, &columns_info, remaining))?;
            cursor_id = next.cursor_id;
            has_more = next.has_more_rows;
            if next.rows.is_empty() {
                break;
            }
            Self::push_rows(&mut rs, next.rows);
        }

        if has_more && cursor_id > 0 {
            self.pending_cursor = Some(PendingCursor {
                sql: sql.to_string(),
                cursor_id,
                columns: columns_info,
            });
            rs.has_more = true;
        } else {
            rs.has_more = false;
        }

        rs.elapsed_ms = start.elapsed().as_millis() as u64;
        Ok(rs)
    }

    fn fetch_more(&mut self, sql: &str, _offset: usize, limit: usize) -> Result<ResultSet> {
        let start = Instant::now();
        let Some(mut pending) = self.pending_cursor.take() else {
            return Ok(ResultSet::empty(0));
        };
        if pending.sql != sql {
            return Ok(ResultSet::empty(0));
        }

        let mut rs = ResultSet::with_columns(Self::convert_columns(&pending.columns));
        rs.source_sql = Some(sql.to_string());

        let target = if limit == 0 { usize::MAX } else { limit };
        let mut has_more = true;
        while has_more && rs.row_count < target {
            let batch = (target - rs.row_count).min(500) as u32;
            let next = self.rt.block_on(self.conn.fetch_more(
                pending.cursor_id,
                &pending.columns,
                batch,
            ))?;
            pending.cursor_id = next.cursor_id;
            has_more = next.has_more_rows;
            if next.rows.is_empty() {
                break;
            }
            Self::push_rows(&mut rs, next.rows);
        }

        if has_more && pending.cursor_id > 0 {
            rs.has_more = true;
            self.pending_cursor = Some(pending);
        } else {
            rs.has_more = false;
        }

        rs.elapsed_ms = start.elapsed().as_millis() as u64;
        Ok(rs)
    }

    fn execute_dml(&mut self, sql: &str) -> Result<u64> {
        let affected = self.rt.block_on(self.conn.execute_dml_sql(sql, &[]))?;
        self.uncommitted_ops += 1;
        Ok(affected)
    }

    fn execute_ddl(&mut self, sql: &str) -> Result<()> {
        self.rt.block_on(self.conn.execute(sql, &[]))?;
        // Oracle では DDL 実行時に暗黙 COMMIT が走る
        self.uncommitted_ops = 0;
        Ok(())
    }

    fn commit(&mut self) -> Result<()> {
        self.rt.block_on(self.conn.commit())?;
        self.uncommitted_ops = 0;
        Ok(())
    }

    fn rollback(&mut self) -> Result<()> {
        self.rt.block_on(self.conn.rollback())?;
        self.uncommitted_ops = 0;
        Ok(())
    }

    fn has_uncommitted_changes(&self) -> bool {
        self.uncommitted_ops > 0
    }

    fn uncommitted_count(&self) -> usize {
        self.uncommitted_ops
    }

    fn list_owners(&mut self) -> Result<Vec<String>> {
        let sql = "SELECT USERNAME FROM ALL_USERS ORDER BY USERNAME";
        let res = self.rt.block_on(self.conn.query(sql, &[]))?;
        let mut owners = Vec::with_capacity(res.rows.len());
        for row in res.rows {
            if let Some(name) = row.get(0).and_then(|v| v.as_str()) {
                owners.push(name.to_string());
            }
        }
        Ok(owners)
    }

    fn list_objects(&mut self, owner: &str, object_type: &ObjectType) -> Result<Vec<SchemaObject>> {
        let obj_type_str = object_type.as_str();
        let sql = match object_type {
            ObjectType::Table | ObjectType::View => {
                "SELECT o.OBJECT_NAME, NVL(c.COMMENTS, '') \
                 FROM ALL_OBJECTS o \
                 LEFT JOIN ALL_TAB_COMMENTS c \
                   ON c.OWNER = o.OWNER AND c.TABLE_NAME = o.OBJECT_NAME \
                 WHERE o.OWNER = :1 AND o.OBJECT_TYPE = :2 \
                 ORDER BY o.OBJECT_NAME"
            }
            _ => {
                "SELECT OBJECT_NAME, '' \
                 FROM ALL_OBJECTS \
                 WHERE OWNER = :1 AND OBJECT_TYPE = :2 \
                 ORDER BY OBJECT_NAME"
            }
        };

        let res = self.rt.block_on(self.conn.query(
            sql,
            &[Value::String(owner.to_string()), Value::String(obj_type_str.to_string())],
        ))?;

        let mut list = Vec::with_capacity(res.rows.len());
        for row in res.rows {
            let name = row.get(0).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let comments = row.get(1).and_then(|v| v.as_str()).unwrap_or("").to_string();
            if !name.is_empty() {
                list.push(SchemaObject {
                    owner: owner.to_string(),
                    name,
                    object_type: object_type.clone(),
                    comments,
                });
            }
        }
        Ok(list)
    }

    fn list_columns(&mut self, owner: &str, table_name: &str) -> Result<Vec<ColumnDetail>> {
        let sql = "SELECT c.COLUMN_NAME, c.DATA_TYPE, \
                   CASE \
                     WHEN c.DATA_PRECISION IS NOT NULL AND c.DATA_SCALE > 0 THEN TO_CHAR(c.DATA_PRECISION) || ',' || TO_CHAR(c.DATA_SCALE) \
                     WHEN c.DATA_PRECISION IS NOT NULL THEN TO_CHAR(c.DATA_PRECISION) \
                     WHEN c.CHAR_LENGTH > 0 THEN TO_CHAR(c.CHAR_LENGTH) \
                     ELSE TO_CHAR(c.DATA_LENGTH) \
                   END AS LEN_TEXT, \
                   c.NULLABLE, \
                   NVL(cm.COMMENTS, '') AS COMMENTS, \
                   CASE WHEN EXISTS ( \
                     SELECT 1 FROM ALL_CONSTRAINTS ac \
                     JOIN ALL_CONS_COLUMNS acc ON acc.OWNER = ac.OWNER AND acc.CONSTRAINT_NAME = ac.CONSTRAINT_NAME \
                     WHERE ac.OWNER = c.OWNER AND ac.TABLE_NAME = c.TABLE_NAME \
                       AND ac.CONSTRAINT_TYPE = 'P' AND acc.COLUMN_NAME = c.COLUMN_NAME \
                   ) THEN 'Y' ELSE 'N' END AS IS_PK \
                   FROM ALL_TAB_COLUMNS c \
                   LEFT JOIN ALL_COL_COMMENTS cm \
                     ON cm.OWNER = c.OWNER AND cm.TABLE_NAME = c.TABLE_NAME AND cm.COLUMN_NAME = c.COLUMN_NAME \
                   WHERE c.OWNER = :1 AND c.TABLE_NAME = :2 \
                   ORDER BY c.COLUMN_ID";

        let res = self.rt.block_on(self.conn.query(
            sql,
            &[Value::String(owner.to_string()), Value::String(table_name.to_string())],
        ))?;

        let mut cols = Vec::with_capacity(res.rows.len());
        for row in res.rows {
            let name = row.get(0).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let data_type = row.get(1).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let length = row.get(2).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let nullable = row.get(3).and_then(|v| v.as_str()).unwrap_or("Y") == "Y";
            let comments = row.get(4).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let is_pk = row.get(5).and_then(|v| v.as_str()).unwrap_or("N") == "Y";
            cols.push(ColumnDetail {
                name,
                data_type,
                length,
                nullable,
                is_pk,
                comments,
            });
        }
        Ok(cols)
    }

    fn get_ddl(&mut self, owner: &str, object_type: &ObjectType, name: &str) -> Result<String> {
        let sql = "SELECT DBMS_METADATA.GET_DDL(:1, :2, :3) FROM DUAL";
        let res = self.rt.block_on(self.conn.query(
            sql,
            &[
                Value::String(object_type.as_str().to_string()),
                Value::String(name.to_string()),
                Value::String(owner.to_string()),
            ],
        ))?;
        let ddl = res
            .rows
            .first()
            .and_then(|r| r.get(0))
            .map(|v| convert_value(v.clone()).display_str().into_owned())
            .unwrap_or_else(|| format!("-- DDL が取得できませんでした: {owner}.{name}"));
        Ok(ddl)
    }

    fn db_info(&self) -> String {
        format!("Oracle ({})", self.target_label)
    }
}

fn convert_value(val: Value) -> CellValue {
    match val {
        Value::Null => CellValue::Null,
        Value::String(s) => CellValue::Text(CompactString::from(s)),
        Value::Bytes(b) => CellValue::Bytes(b.into()),
        Value::Integer(i) => CellValue::Int(i),
        Value::Float(f) => CellValue::Float(f),
        Value::Number(n) => CellValue::Decimal(CompactString::from(n.as_str())),
        Value::Date(d) => {
            let mut s = CompactString::default();
            let _ = write!(
                s,
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                d.year, d.month, d.day, d.hour, d.minute, d.second
            );
            CellValue::DateTime(s)
        }
        Value::Timestamp(ts) => {
            let mut s = CompactString::default();
            let _ = write!(
                s,
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                ts.year, ts.month, ts.day, ts.hour, ts.minute, ts.second
            );
            if ts.microsecond > 0 {
                let _ = write!(s, ".{:06}", ts.microsecond);
            }
            if ts.has_timezone() {
                let _ = write!(s, " {:+03}:{:02}", ts.tz_hour_offset, ts.tz_minute_offset);
            }
            CellValue::DateTime(s)
        }
        Value::Boolean(b) => CellValue::Bool(b),
        Value::RowId(r) => match r.to_string() {
            Some(s) => CellValue::Text(CompactString::from(s)),
            None => CellValue::Null,
        },
        other => CellValue::Text(CompactString::from(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ququ_core::EnvironmentTier;

    #[test]
    fn builds_direct_oracle_config() {
        let mut profile =
            ConnectionProfile::new_template(EnvironmentTier::Development, ququ_core::DbType::Oracle);
        profile.host = "db.internal".to_string();
        profile.port = 1521;
        profile.service_name = "FREEPDB1".to_string();
        profile.username = "SCOTT".to_string();

        let creds = ConnectCredentials {
            password: Some("tiger".to_string()),
        };

        let cfg = build_oracle_config(&profile, &creds, None).unwrap();
        assert_eq!(cfg.host, "db.internal");
        assert_eq!(cfg.port, 1521);
        assert_eq!(cfg.service.service_name(), Some("FREEPDB1"));
        assert_eq!(cfg.username, "SCOTT");
    }

    #[test]
    #[ignore = "Oracle 実機接続環境が必要な統合テスト"]
    fn live_oracle_smoke_test() {
        let host = std::env::var("QUQU_ORACLE_HOST").unwrap();
        let service = std::env::var("QUQU_ORACLE_SERVICE").unwrap();
        let user = std::env::var("QUQU_ORACLE_USER").unwrap();
        let password = std::env::var("QUQU_ORACLE_PASS").unwrap();

        let mut profile =
            ConnectionProfile::new_template(EnvironmentTier::Development, ququ_core::DbType::Oracle);
        profile.host = host;
        profile.service_name = service;
        profile.username = user;

        let creds = ConnectCredentials {
            password: Some(password),
        };
        let mut sess = OracleSession::connect(&profile, &creds, None).unwrap();
        let rs = sess.execute_query("SELECT 1 FROM DUAL", 10).unwrap();
        assert_eq!(rs.row_count, 1);
    }
}
