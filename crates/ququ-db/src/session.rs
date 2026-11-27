use anyhow::Result;
use ququ_core::{
    ColumnDetail, ConnectCredentials, ConnectionProfile, ObjectType, ResultSet, SchemaObject,
};

/// 各 RDBMS ドライバが実装する統一セッションインターフェース。
pub trait DbSession: Send {
    fn execute_query(&mut self, sql: &str, fetch_limit: usize) -> Result<ResultSet>;
    fn fetch_more(&mut self, sql: &str, offset: usize, limit: usize) -> Result<ResultSet>;
    fn execute_dml(&mut self, sql: &str) -> Result<u64>;
    fn execute_ddl(&mut self, sql: &str) -> Result<()>;
    fn commit(&mut self) -> Result<()>;
    fn rollback(&mut self) -> Result<()>;
    fn has_uncommitted_changes(&self) -> bool;
    fn uncommitted_count(&self) -> usize;

    fn list_owners(&mut self) -> Result<Vec<String>>;
    fn list_objects(&mut self, owner: &str, object_type: &ObjectType) -> Result<Vec<SchemaObject>>;
    fn list_columns(&mut self, owner: &str, table_name: &str) -> Result<Vec<ColumnDetail>>;
    fn get_ddl(&mut self, owner: &str, object_type: &ObjectType, name: &str) -> Result<String>;
    fn db_info(&self) -> String;
}

/// 接続プロファイルと資格情報から `DbSession` を構築するファクトリ。
pub trait DbSessionFactory: Send + Sync {
    fn connect(
        &self,
        profile: &ConnectionProfile,
        credentials: &ConnectCredentials,
        tns_admin_override: Option<&str>,
    ) -> Result<Box<dyn DbSession>>;
}
