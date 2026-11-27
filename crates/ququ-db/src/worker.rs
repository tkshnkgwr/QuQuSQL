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

pub enum DbCommand {
    Connect {
        profile: ConnectionProfile,
        credentials: ConnectCredentials,
        tns_admin_override: Option<String>,
    },
    Disconnect,
    ExecuteSql {
        sql: String,
        fetch_limit: usize,
        allow_write: bool,
    },
    FetchMore {
        sql: String,
        offset: usize,
        limit: usize,
    },
    Commit,
    Rollback,
    LoadOwners,
    LoadObjects {
        owner: String,
        object_type: ObjectType,
    },
    LoadColumns {
        owner: String,
        table_name: String,
    },
    LoadDdl {
        owner: String,
        object_type: ObjectType,
        name: String,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum DbEvent {
    Connected {
        profile_id: String,
        db_info: String,
    },
    Disconnected,
    QueryFinished {
        sql: String,
        outcome: QueryOutcome,
        has_uncommitted: bool,
        uncommitted_count: usize,
    },
    FetchedMore {
        result: ResultSet,
    },
    CommitFinished,
    RollbackFinished,
    OwnersLoaded(Vec<String>),
    ObjectsLoaded {
        owner: String,
        object_type: ObjectType,
        objects: Vec<SchemaObject>,
    },
    ColumnsLoaded {
        owner: String,
        table_name: String,
        columns: Vec<ColumnDetail>,
    },
    DdlLoaded {
        name: String,
        ddl: String,
    },
    Error {
        context: String,
        message: String,
    },
}

/// UI スレッドとは独立した専用ワーカーで DB セッションを管理する。
pub struct DbWorker {
    cmd_tx: Sender<DbCommand>,
    event_rx: Receiver<DbEvent>,
    handle: Option<JoinHandle<()>>,
}

impl DbWorker {
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
            .expect("failed to spawn ququ-db-worker");

        Self {
            cmd_tx,
            event_rx,
            handle: Some(handle),
        }
    }

    pub fn send(&self, cmd: DbCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

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

fn send_event(tx: &Sender<DbEvent>, repaint: &dyn Fn(), event: DbEvent) {
    let _ = tx.send(event);
    repaint();
}

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
                let start = Instant::now();

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

fn truncate_sql(sql: &str) -> String {
    let single_line = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() > 60 {
        let prefix: String = single_line.chars().take(60).collect();
        format!("{prefix}...")
    } else {
        single_line
    }
}
