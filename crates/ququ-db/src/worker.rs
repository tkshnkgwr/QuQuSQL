//! バックグラウンドスレッドで DB 接続を所有し、非同期的にコマンドを処理するワーカーモジュール。

use crate::session::{DbSession, DbSessionFactory};
use crossbeam_channel::{Receiver, Sender, unbounded};
use ququ_core::{
    ColumnDetail, ConnectCredentials, ConnectionProfile, ObjectType, QueryOutcome, ResultSet,
    SchemaObject,
};
use ququ_sql::{StatementKind, analyze_statement};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

/// UI スレッドから DB ワーカーへ送信する処理要求コマンド。
pub enum DbCommand {
    /// 指定したプロファイルと認証情報で DB へ接続する
    Connect {
        /// 接続プロファイル
        profile: ConnectionProfile,
        /// パスワードを含む一時認証情報
        credentials: ConnectCredentials,
        /// `tnsnames.ora` の探索パス上書き指定
        tns_admin_override: Option<String>,
    },
    /// 現在の接続を切断する
    Disconnect,
    /// 1 文の SQL を実行する
    ExecuteSql {
        /// 実行する SQL 文
        sql: String,
        /// SELECT 時の初期フェッチ上限行数
        fetch_limit: usize,
        /// 更新系（DML / DDL / COMMIT）実行を許可するか
        allow_write: bool,
    },
    /// 結果セットの続きを追加フェッチする
    FetchMore {
        /// 元の SELECT 文
        sql: String,
        /// 取得済み行数オフセット
        offset: usize,
        /// 追加取得する最大行数（0 なら全件）
        limit: usize,
    },
    /// 明示的コミットを実行する
    Commit,
    /// 明示的ロールバックを実行する
    Rollback,
    /// スキーマ（owner）一覧を取得する
    LoadOwners,
    /// 指定スキーマ・種別のオブジェクト一覧を取得する
    LoadObjects {
        /// スキーマ名
        owner: String,
        /// オブジェクト種別
        object_type: ObjectType,
    },
    /// 指定テーブル・ビューのカラム詳細一覧を取得する
    LoadColumns {
        /// スキーマ名
        owner: String,
        /// テーブルまたはビュー名
        table_name: String,
    },
    /// 指定オブジェクトの DDL 文を取得する
    LoadDdl {
        /// スキーマ名
        owner: String,
        /// オブジェクト種別
        object_type: ObjectType,
        /// オブジェクト名
        name: String,
    },
    /// ワーカースレッドを終了する
    Shutdown,
}

/// DB ワーカーから UI スレッドへ返却する実行結果イベント。
#[derive(Debug, Clone)]
pub enum DbEvent {
    /// DB 接続完了
    Connected {
        /// 接続したプロファイル ID
        profile_id: String,
        /// 接続先表示情報
        db_info: String,
    },
    /// DB 切断完了
    Disconnected,
    /// SQL 実行完了
    QueryFinished {
        /// 実行した SQL 文
        sql: String,
        /// 実行結果（行セット / 影響行数 / メッセージ）
        outcome: QueryOutcome,
        /// DDL 文を実行したかどうか（サイドバー自動再読込判定用）
        is_ddl: bool,
        /// 未コミット変更が残っているか
        has_uncommitted: bool,
        /// 未コミット操作件数
        uncommitted_count: usize,
    },
    /// 追加フェッチ完了
    FetchedMore {
        /// 追加取得した結果セット
        result: ResultSet,
    },
    /// コミット完了
    CommitFinished,
    /// ロールバック完了
    RollbackFinished,
    /// スキーマ一覧取得完了
    OwnersLoaded(Vec<String>),
    /// オブジェクト一覧取得完了
    ObjectsLoaded {
        /// スキーマ名
        owner: String,
        /// オブジェクト種別
        object_type: ObjectType,
        /// 取得したオブジェクト一覧
        objects: Vec<SchemaObject>,
    },
    /// カラム詳細取得完了
    ColumnsLoaded {
        /// スキーマ名
        owner: String,
        /// テーブル名
        table_name: String,
        /// 取得したカラム詳細一覧
        columns: Vec<ColumnDetail>,
    },
    /// DDL 取得完了
    DdlLoaded {
        /// 対象オブジェクト名
        name: String,
        /// 取得した DDL 文字列
        ddl: String,
    },
    /// DB 操作中のエラー通知
    Error {
        /// エラー発生元の処理コンテキスト
        context: String,
        /// エラーメッセージ詳細
        message: String,
    },
}

/// UI スレッドとは独立した専用ワーカースレッドで `DbSession` を所有・実行する管理構造体。
pub struct DbWorker {
    cmd_tx: Sender<DbCommand>,
    event_rx: Receiver<DbEvent>,
    handle: Option<JoinHandle<()>>,
}

impl DbWorker {
    /// 専用ワーカースレッドを起動し、イベント通知時に `repaint_callback` を呼び出す。
    pub fn spawn(
        factory: Arc<dyn DbSessionFactory>,
        repaint_callback: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (cmd_tx, cmd_rx) = unbounded::<DbCommand>();
        let (event_tx, event_rx) = unbounded::<DbEvent>();

        let handle = thread::Builder::new()
            .name("ququ-db-worker".to_string())
            .spawn(move || {
                worker_loop(factory, cmd_rx, event_tx, repaint_callback);
            })
            .expect("ququ-db-worker スレッドの起動に失敗しました");

        Self {
            cmd_tx,
            event_rx,
            handle: Some(handle),
        }
    }

    /// コマンドを非同期キューへ送信する。
    pub fn send(&self, cmd: DbCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    /// 完了したイベントをノンブロッキングで 1 件取り出す。
    pub fn try_recv(&self) -> Option<DbEvent> {
        self.event_rx.try_recv().ok()
    }
}

impl Drop for DbWorker {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(DbCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// イベントを送信し、UI の再描画を即座に要求する。
fn send_event(tx: &Sender<DbEvent>, repaint: &dyn Fn(), event: DbEvent) {
    let _ = tx.send(event);
    repaint();
}

/// ワーカースレッドのメインイベントループ。
fn worker_loop(
    factory: Arc<dyn DbSessionFactory>,
    cmd_rx: Receiver<DbCommand>,
    event_tx: Sender<DbEvent>,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    let mut session: Option<Box<dyn DbSession>> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            DbCommand::Shutdown => break,
            DbCommand::Connect {
                profile,
                credentials,
                tns_admin_override,
            } => {
                session = None;
                match factory.connect(&profile, &credentials, tns_admin_override.as_deref()) {
                    Ok(s) => {
                        let db_info = s.db_info();
                        session = Some(s);
                        send_event(
                            &event_tx,
                            repaint.as_ref(),
                            DbEvent::Connected {
                                profile_id: profile.id,
                                db_info,
                            },
                        );
                    }
                    Err(e) => {
                        send_event(
                            &event_tx,
                            repaint.as_ref(),
                            DbEvent::Error {
                                context: "接続エラー".to_string(),
                                message: format!("{e:#}"),
                            },
                        );
                    }
                }
            }
            DbCommand::Disconnect => {
                session = None;
                send_event(&event_tx, repaint.as_ref(), DbEvent::Disconnected);
            }
            DbCommand::ExecuteSql {
                sql,
                fetch_limit,
                allow_write,
            } => {
                let Some(sess) = session.as_mut() else {
                    send_event(
                        &event_tx,
                        repaint.as_ref(),
                        DbEvent::Error {
                            context: "SQL実行".to_string(),
                            message: "データベースに接続されていません。".to_string(),
                        },
                    );
                    continue;
                };

                let kind = analyze_statement(&sql);
                let is_ddl = kind == StatementKind::Ddl;
                let start = Instant::now();

                // UI 側ガードに加え、ワーカー層でも Read-Only 時の更新系実行を二重防衛する
                let res: anyhow::Result<QueryOutcome> = match kind {
                    StatementKind::Query | StatementKind::Other => {
                        sess.execute_query(&sql, fetch_limit).map(QueryOutcome::Rows)
                    }
                    StatementKind::Dml => {
                        if !allow_write {
                            Err(anyhow::anyhow!(
                                "読み取り専用モードのため DML の実行はブロックされました。"
                            ))
                        } else {
                            sess.execute_dml(&sql).map(|rows| QueryOutcome::Affected {
                                rows,
                                elapsed_ms: start.elapsed().as_millis() as u64,
                            })
                        }
                    }
                    StatementKind::Ddl => {
                        if !allow_write {
                            Err(anyhow::anyhow!(
                                "読み取り専用モードのため DDL の実行はブロックされました。"
                            ))
                        } else {
                            sess.execute_ddl(&sql).map(|()| QueryOutcome::Message {
                                text: "DDL を実行しました。".to_string(),
                                elapsed_ms: start.elapsed().as_millis() as u64,
                            })
                        }
                    }
                    StatementKind::Commit => {
                        if !allow_write {
                            Err(anyhow::anyhow!(
                                "読み取り専用モードのため COMMIT はブロックされました。"
                            ))
                        } else {
                            sess.commit().map(|()| QueryOutcome::Message {
                                text: "コミットしました。".to_string(),
                                elapsed_ms: start.elapsed().as_millis() as u64,
                            })
                        }
                    }
                    StatementKind::Rollback => sess.rollback().map(|()| QueryOutcome::Message {
                        text: "ロールバックしました。".to_string(),
                        elapsed_ms: start.elapsed().as_millis() as u64,
                    }),
                };

                match res {
                    Ok(outcome) => send_event(
                        &event_tx,
                        repaint.as_ref(),
                        DbEvent::QueryFinished {
                            sql,
                            outcome,
                            is_ddl,
                            has_uncommitted: sess.has_uncommitted_changes(),
                            uncommitted_count: sess.uncommitted_count(),
                        },
                    ),
                    Err(e) => send_event(
                        &event_tx,
                        repaint.as_ref(),
                        DbEvent::Error {
                            context: format!("SQL実行: {}", truncate_sql(&sql)),
                            message: format!("{e:#}"),
                        },
                    ),
                }
            }
            DbCommand::FetchMore { sql, offset, limit } => {
                let Some(sess) = session.as_mut() else {
                    continue;
                };
                match sess.fetch_more(&sql, offset, limit) {
                    Ok(result) => {
                        send_event(&event_tx, repaint.as_ref(), DbEvent::FetchedMore { result })
                    }
                    Err(e) => send_event(
                        &event_tx,
                        repaint.as_ref(),
                        DbEvent::Error {
                            context: "追加フェッチ".to_string(),
                            message: format!("{e:#}"),
                        },
                    ),
                }
            }
            DbCommand::Commit => {
                let Some(sess) = session.as_mut() else {
                    continue;
                };
                match sess.commit() {
                    Ok(()) => send_event(&event_tx, repaint.as_ref(), DbEvent::CommitFinished),
                    Err(e) => send_event(
                        &event_tx,
                        repaint.as_ref(),
                        DbEvent::Error {
                            context: "コミット".to_string(),
                            message: format!("{e:#}"),
                        },
                    ),
                }
            }
            DbCommand::Rollback => {
                let Some(sess) = session.as_mut() else {
                    continue;
                };
                match sess.rollback() {
                    Ok(()) => send_event(&event_tx, repaint.as_ref(), DbEvent::RollbackFinished),
                    Err(e) => send_event(
                        &event_tx,
                        repaint.as_ref(),
                        DbEvent::Error {
                            context: "ロールバック".to_string(),
                            message: format!("{e:#}"),
                        },
                    ),
                }
            }
            DbCommand::LoadOwners => {
                let Some(sess) = session.as_mut() else {
                    continue;
                };
                match sess.list_owners() {
                    Ok(owners) => {
                        send_event(&event_tx, repaint.as_ref(), DbEvent::OwnersLoaded(owners))
                    }
                    Err(e) => send_event(
                        &event_tx,
                        repaint.as_ref(),
                        DbEvent::Error {
                            context: "スキーマ一覧取得".to_string(),
                            message: format!("{e:#}"),
                        },
                    ),
                }
            }
            DbCommand::LoadObjects { owner, object_type } => {
                let Some(sess) = session.as_mut() else {
                    continue;
                };
                match sess.list_objects(&owner, &object_type) {
                    Ok(objects) => send_event(
                        &event_tx,
                        repaint.as_ref(),
                        DbEvent::ObjectsLoaded {
                            owner,
                            object_type,
                            objects,
                        },
                    ),
                    Err(e) => send_event(
                        &event_tx,
                        repaint.as_ref(),
                        DbEvent::Error {
                            context: "オブジェクト一覧取得".to_string(),
                            message: format!("{e:#}"),
                        },
                    ),
                }
            }
            DbCommand::LoadColumns { owner, table_name } => {
                let Some(sess) = session.as_mut() else {
                    continue;
                };
                match sess.list_columns(&owner, &table_name) {
                    Ok(columns) => send_event(
                        &event_tx,
                        repaint.as_ref(),
                        DbEvent::ColumnsLoaded {
                            owner,
                            table_name,
                            columns,
                        },
                    ),
                    Err(e) => send_event(
                        &event_tx,
                        repaint.as_ref(),
                        DbEvent::Error {
                            context: "カラム情報取得".to_string(),
                            message: format!("{e:#}"),
                        },
                    ),
                }
            }
            DbCommand::LoadDdl {
                owner,
                object_type,
                name,
            } => {
                let Some(sess) = session.as_mut() else {
                    continue;
                };
                match sess.get_ddl(&owner, &object_type, &name) {
                    Ok(ddl) => send_event(
                        &event_tx,
                        repaint.as_ref(),
                        DbEvent::DdlLoaded { name, ddl },
                    ),
                    Err(e) => send_event(
                        &event_tx,
                        repaint.as_ref(),
                        DbEvent::Error {
                            context: "DDL取得".to_string(),
                            message: format!("{e:#}"),
                        },
                    ),
                }
            }
        }
    }
}

/// エラーログ表示向けに SQL 文字列を先頭 60 文字までに省略整形する。
fn truncate_sql(sql: &str) -> String {
    let single_line = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() > 60 {
        let prefix: String = single_line.chars().take(60).collect();
        format!("{prefix}...")
    } else {
        single_line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ququ_core::{DbType, EnvironmentTier};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    struct MockSession {
        uncommitted: usize,
    }

    impl DbSession for MockSession {
        fn execute_query(&mut self, sql: &str, _fetch_limit: usize) -> anyhow::Result<ResultSet> {
            let mut rs = ResultSet::empty(1);
            rs.source_sql = Some(sql.to_string());
            Ok(rs)
        }
        fn fetch_more(&mut self, _sql: &str, _offset: usize, _limit: usize) -> anyhow::Result<ResultSet> {
            Ok(ResultSet::empty(1))
        }
        fn execute_dml(&mut self, _sql: &str) -> anyhow::Result<u64> {
            self.uncommitted += 1;
            Ok(1)
        }
        fn execute_ddl(&mut self, _sql: &str) -> anyhow::Result<()> {
            self.uncommitted = 0;
            Ok(())
        }
        fn commit(&mut self) -> anyhow::Result<()> {
            self.uncommitted = 0;
            Ok(())
        }
        fn rollback(&mut self) -> anyhow::Result<()> {
            self.uncommitted = 0;
            Ok(())
        }
        fn has_uncommitted_changes(&self) -> bool {
            self.uncommitted > 0
        }
        fn uncommitted_count(&self) -> usize {
            self.uncommitted
        }
        fn list_owners(&mut self) -> anyhow::Result<Vec<String>> {
            Ok(vec!["MOCK".to_string()])
        }
        fn list_objects(&mut self, _owner: &str, _object_type: &ObjectType) -> anyhow::Result<Vec<SchemaObject>> {
            Ok(Vec::new())
        }
        fn list_columns(&mut self, _owner: &str, _table_name: &str) -> anyhow::Result<Vec<ColumnDetail>> {
            Ok(Vec::new())
        }
        fn get_ddl(&mut self, owner: &str, _object_type: &ObjectType, name: &str) -> anyhow::Result<String> {
            Ok(format!("CREATE TABLE {owner}.{name} (ID NUMBER)"))
        }
        fn db_info(&self) -> String {
            "MockDB".to_string()
        }
    }

    struct MockFactory;

    impl DbSessionFactory for MockFactory {
        fn connect(
            &self,
            _profile: &ConnectionProfile,
            _credentials: &ConnectCredentials,
            _tns_admin_override: Option<&str>,
        ) -> anyhow::Result<Box<dyn DbSession>> {
            Ok(Box::new(MockSession { uncommitted: 0 }))
        }
    }

    fn recv_event(worker: &DbWorker) -> DbEvent {
        for _ in 0..100 {
            if let Some(ev) = worker.try_recv() {
                return ev;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("タイムアウト: DbWorker からイベントが返りませんでした");
    }

    #[test]
    fn worker_blocks_writes_when_readonly_and_tracks_ddl_flag() {
        let repaints = Arc::new(AtomicUsize::new(0));
        let repaints_clone = Arc::clone(&repaints);
        let worker = DbWorker::spawn(
            Arc::new(MockFactory),
            Arc::new(move || {
                repaints_clone.fetch_add(1, Ordering::SeqCst);
            }),
        );

        let prof = ConnectionProfile::new_template(EnvironmentTier::Production, DbType::Oracle);
        worker.send(DbCommand::Connect {
            profile: prof,
            credentials: ConnectCredentials::default(),
            tns_admin_override: None,
        });
        assert!(matches!(recv_event(&worker), DbEvent::Connected { .. }));

        // allow_write = false のとき DML / DDL / COMMIT はワーカー層で拒否される
        for dangerous_sql in [
            "INSERT INTO T VALUES (1)",
            "CREATE TABLE T (ID INT)",
            "COMMIT",
        ] {
            worker.send(DbCommand::ExecuteSql {
                sql: dangerous_sql.to_string(),
                fetch_limit: 1000,
                allow_write: false,
            });
            assert!(matches!(recv_event(&worker), DbEvent::Error { .. }));
        }

        // allow_write = true で DDL を実行すると is_ddl = true が返る
        worker.send(DbCommand::ExecuteSql {
            sql: "CREATE TABLE TEST_TBL (ID NUMBER)".to_string(),
            fetch_limit: 1000,
            allow_write: true,
        });
        match recv_event(&worker) {
            DbEvent::QueryFinished { is_ddl, .. } => assert!(is_ddl),
            other => panic!("unexpected event: {other:?}"),
        }

        // LoadDdl で DDL 文字列が取得できる
        worker.send(DbCommand::LoadDdl {
            owner: "MOCK".to_string(),
            object_type: ObjectType::Table,
            name: "TEST_TBL".to_string(),
        });
        match recv_event(&worker) {
            DbEvent::DdlLoaded { name, ddl } => {
                assert_eq!(name, "TEST_TBL");
                assert!(ddl.contains("CREATE TABLE MOCK.TEST_TBL"));
            }
            other => panic!("unexpected event: {other:?}"),
        }

        assert!(repaints.load(Ordering::SeqCst) >= 5);
    }
}
