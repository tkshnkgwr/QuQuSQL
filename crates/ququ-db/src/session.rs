//! 各 RDBMS ドライバが実装する統一セッションおよびファクトリのトレイト定義。

use anyhow::Result;
use ququ_core::{
    ColumnDetail, ConnectCredentials, ConnectionProfile, ObjectType, ResultSet, SchemaObject,
};

/// 各 RDBMS ドライバが実装する統一セッションインターフェース。
pub trait DbSession: Send {
    /// SELECT 等の参照系クエリを実行し、最大 `fetch_limit` 件（0 なら全件）を返す。
    fn execute_query(&mut self, sql: &str, fetch_limit: usize) -> Result<ResultSet>;
    /// 既存クエリの続きを指定オフセット・件数で追加取得する。
    fn fetch_more(&mut self, sql: &str, offset: usize, limit: usize) -> Result<ResultSet>;
    /// INSERT / UPDATE / DELETE 等の DML を実行し、影響行数を返す。
    fn execute_dml(&mut self, sql: &str) -> Result<u64>;
    /// CREATE / ALTER / DROP 等の DDL を実行する。
    fn execute_ddl(&mut self, sql: &str) -> Result<()>;
    /// 現在のトランザクションを明示的にコミットする。
    fn commit(&mut self) -> Result<()>;
    /// 現在のトランザクションを明示的にロールバックする。
    fn rollback(&mut self) -> Result<()>;
    /// 未コミットの変更が残っているかを返す。
    fn has_uncommitted_changes(&self) -> bool;
    /// 未コミット操作件数を返す。
    fn uncommitted_count(&self) -> usize;

    /// 利用可能なスキーマ（owner）一覧を取得する。
    fn list_owners(&mut self) -> Result<Vec<String>>;
    /// 指定スキーマ・種別のオブジェクト一覧を取得する。
    fn list_objects(&mut self, owner: &str, object_type: &ObjectType) -> Result<Vec<SchemaObject>>;
    /// 指定テーブル・ビューのカラム詳細を取得する。
    fn list_columns(&mut self, owner: &str, table_name: &str) -> Result<Vec<ColumnDetail>>;
    /// 指定オブジェクトの CREATE DDL 文を取得する。
    fn get_ddl(&mut self, owner: &str, object_type: &ObjectType, name: &str) -> Result<String>;
    /// OSqlGrid 直接編集向けにテーブルデータと行識別キー（ROWID 等）を取得する。
    fn load_table_for_edit(
        &mut self,
        owner: &str,
        table_name: &str,
        where_clause: &str,
        order_by: &str,
        fetch_limit: usize,
    ) -> Result<ququ_core::EditableTableData>;
    /// パラメータ化 DML バッチを SAVEPOINT 保護下で適用し、合計影響行数を返す（自動 COMMIT しない）。
    fn apply_dml_batch(&mut self, dmls: &[ququ_core::ParameterizedDml]) -> Result<usize>;
    /// 接続先を表す表示用情報文字列を返す。
    fn db_info(&self) -> String;
}

/// 接続プロファイルと資格情報から `DbSession` を構築するファクトリ。
pub trait DbSessionFactory: Send + Sync {
    /// データベースへの接続を確立し、セッションオブジェクトを返す。
    fn connect(
        &self,
        profile: &ConnectionProfile,
        credentials: &ConnectCredentials,
        tns_admin_override: Option<&str>,
    ) -> Result<Box<dyn DbSession>>;
}
