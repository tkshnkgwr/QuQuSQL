//! UI 全体で共有する状態モデルおよびアプリ本体へのコマンド定義モジュール。

use ququ_core::{
    AppConfig, ColumnDetail, ConnectCredentials, ConnectionProfile, EnvironmentTier, ObjectType,
    QueryLogEntry, ResultSet, SchemaObject, StoragePaths, WindowState,
};
use std::path::PathBuf;

/// SQL エディタ 1 タブ分の保持状態。
#[derive(Debug, Clone)]
pub struct SqlTab {
    /// タブの一意 ID
    pub id: usize,
    /// タブ見出し文字列
    pub title: String,
    /// 紐づくファイルパス（未保存タブは `None`）
    pub file_path: Option<PathBuf>,
    /// エディタ本文
    pub text: String,
    /// 未保存の変更があるか
    pub dirty: bool,
    /// カーソル位置（バイトオフセット）
    pub cursor_byte_offset: usize,
    /// カーソル行番号（1 始まり）
    pub cursor_line: usize,
    /// カーソル列番号（1 始まり）
    pub cursor_col: usize,
    /// 選択範囲のバイトオフセット区間
    pub selection_byte_range: Option<(usize, usize)>,
    /// ステータスバーに表示する文字コード・改行コード表記
    pub encoding_label: String,
}

impl SqlTab {
    /// 指定タイトルと初期テキストで新しいタブを生成する。
    pub fn new(id: usize, title: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            file_path: None,
            text: text.into(),
            dirty: false,
            cursor_byte_offset: 0,
            cursor_line: 1,
            cursor_col: 1,
            selection_byte_range: None,
            encoding_label: "UTF-8, CRLF".to_string(),
        }
    }
}

/// エディタの検索・置換バーの状態。
#[derive(Debug, Clone, Default)]
pub struct FindReplaceState {
    /// パネルが開いているか
    pub open: bool,
    /// 置換行を表示するか
    pub show_replace: bool,
    /// 検索文字列
    pub query: String,
    /// 置換後文字列
    pub replace_with: String,
    /// 大文字・小文字を区別するか
    pub case_sensitive: bool,
    /// 正規表現モードが有効か
    pub use_regex: bool,
    /// 検索・置換結果のメッセージ
    pub status_msg: String,
    /// ジャンプ時にエディタへ反映待ちの選択範囲（バイトオフセット）
    pub pending_select_range: Option<(usize, usize)>,
}

/// 左下ペインのサブタブ種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    /// 実行ログタブ
    Log,
    /// クエリ結果グリッドタブ
    Grid,
}

/// 確認モーダルで承認された後に実行するアクション種別。
#[derive(Debug, Clone)]
pub enum ConfirmAction {
    /// 本番環境または確認有効環境でのコミット実行
    ProductionCommit {
        /// 未コミット操作件数
        uncommitted_count: usize,
    },
    /// 本番環境または確認有効環境での更新・定義変更 SQL 実行
    ProductionSql {
        /// 実行予定の SQL 文
        sql: String,
        /// DDL かどうか
        is_ddl: bool,
        /// 未コミット変更が残っているか
        has_uncommitted: bool,
        /// 未コミット操作件数
        uncommitted_count: usize,
    },
    /// WHERE 句なしの UPDATE / DELETE 実行
    DmlWithoutWhere {
        /// 実行予定の SQL 文
        sql: String,
    },
    /// 未コミット変更が残っている状態での DDL 実行（暗黙コミット警告）
    ImplicitCommitDdl {
        /// 実行予定の DDL 文
        sql: String,
        /// 未コミット操作件数
        uncommitted_count: usize,
    },
    /// 未コミット変更がある状態でのアプリ終了
    ExitWithUncommitted {
        /// 未コミット操作件数
        uncommitted_count: usize,
    },
}

/// 誤操作防止のための確認モーダル表示状態。
#[derive(Debug, Clone)]
pub struct ConfirmModal {
    /// モーダルタイトル
    pub title: String,
    /// 警告・確認メッセージ
    pub message: String,
    /// 実行予定 SQL のプレビュー
    pub sql_preview: Option<String>,
    /// 本番環境や破壊的操作など危険度が高いか
    pub is_danger: bool,
    /// 確定ボタンの表示ラベル
    pub confirm_label: String,
    /// 承認時に実行するアクション
    pub action: ConfirmAction,
}

/// 接続・ログインダイアログの UI 状態。
#[derive(Debug, Clone)]
pub struct LoginDialogState {
    /// ダイアログ表示フラグ
    pub open: bool,
    /// 選択中の接続プロファイル ID
    pub selected_profile_id: Option<String>,
    /// 都度入力されたパスワード（メモリ内一時保持）
    pub password_input: String,
    /// 新規作成・編集中のプロファイル（編集モード時のみ `Some`）
    pub editing_profile: Option<ConnectionProfile>,
    /// 新規作成中か既存編集中か
    pub is_creating_new: bool,
    /// 接続エラー時のメッセージ
    pub error_msg: Option<String>,
    /// 本番セクションの展開状態（デフォルトは安全のため折りたたみ）
    pub production_expanded: bool,
}

impl Default for LoginDialogState {
    fn default() -> Self {
        Self {
            open: true,
            selected_profile_id: Some("default-sqlite-memory".to_string()),
            password_input: String::new(),
            editing_profile: None,
            is_creating_new: false,
            error_msg: None,
            production_expanded: false,
        }
    }
}

/// UI 層から `ququ-app`（DB ワーカー連携層）へ発行するアクション。
#[derive(Debug)]
pub enum AppAction {
    /// 指定プロファイルへ接続
    Connect {
        /// 接続プロファイル
        profile: ConnectionProfile,
        /// パスワードを含む一時認証情報
        credentials: ConnectCredentials,
        /// `tnsnames.ora` の探索パス上書き指定
        tns_admin_override: Option<String>,
    },
    /// 切断
    Disconnect,
    /// SQL 実行
    ExecuteSql {
        /// 実行する SQL 文
        sql: String,
        /// 更新系実行を許可するか
        allow_write: bool,
    },
    /// 結果セットの追加フェッチ
    FetchMore {
        /// 元の SELECT 文
        sql: String,
        /// 取得済み行数オフセット
        offset: usize,
        /// 追加取得する最大行数（0 なら全件）
        limit: usize,
    },
    /// コミット
    Commit,
    /// ロールバック
    Rollback,
    /// スキーマ一覧再読込
    LoadOwners,
    /// オブジェクト一覧読込
    LoadObjects {
        /// スキーマ名
        owner: String,
        /// オブジェクト種別
        object_type: ObjectType,
    },
    /// カラム詳細読込
    LoadColumns {
        /// スキーマ名
        owner: String,
        /// テーブルまたはビュー名
        table_name: String,
    },
    /// DDL を取得してエディタへ貼り付ける
    LoadDdl {
        /// スキーマ名
        owner: String,
        /// オブジェクト種別
        object_type: ObjectType,
        /// オブジェクト名
        name: String,
    },
    /// カラム詳細を取得した後、全項目 SELECT 文をエディタへ挿入する
    InsertFullSelect {
        /// スキーマ名
        owner: String,
        /// テーブル名
        table_name: String,
    },
    /// 接続プロファイル保存
    SaveProfiles,
    /// アプリ設定保存
    SaveConfig,
    /// 未コミット確認後の強制終了実行
    ConfirmExitClose,
}

/// QuQuSQL GUI 全体のアプリケーション状態。
pub struct UiState {
    /// Cargo.toml からビルド時に埋め込まれるアプリバージョン（ハードコーディング禁止）
    pub app_version: String,
    /// アプリ全体設定
    pub config: AppConfig,
    /// ウィンドウサイズ・ペイン分割比率
    pub window_state: WindowState,
    /// 設定ファイル保存パス
    pub storage: Option<StoragePaths>,
    /// 登録済み接続プロファイル一覧
    pub profiles: Vec<ConnectionProfile>,

    /// 現在接続中のプロファイル
    pub active_profile: Option<ConnectionProfile>,
    /// 接続先情報文字列
    pub db_info: Option<String>,
    /// 現在のセッションが読み取り専用状態か
    pub read_only_session: bool,
    /// 未コミット操作件数
    pub uncommitted_count: usize,
    /// 非同期 DB 処理実行中フラグ
    pub busy: bool,

    /// 開いている SQL エディタタブ一覧
    pub tabs: Vec<SqlTab>,
    /// アクティブなタブのインデックス
    pub active_tab: usize,
    /// 次に割り当てるタブ ID
    pub next_tab_id: usize,
    /// 検索・置換パネル状態
    pub find_replace: FindReplaceState,

    /// 左下ペインの選択中サブタブ
    pub bottom_tab: BottomTab,
    /// SQL 実行ログ一覧
    pub logs: Vec<QueryLogEntry>,
    /// 最新の SELECT 結果セット
    pub result: Option<ResultSet>,
    /// 結果グリッドで選択中のセル `(row, col)`
    pub selected_cell: Option<(usize, usize)>,

    /// スキーマ一覧
    pub owners: Vec<String>,
    /// 選択中スキーマ
    pub selected_owner: String,
    /// 選択中オブジェクト種別
    pub selected_object_type: ObjectType,
    /// オブジェクト一覧のフィルタ文字列
    pub object_filter: String,
    /// 取得済みオブジェクト一覧
    pub objects: Vec<SchemaObject>,
    /// 選択中のオブジェクトインデックス
    pub selected_object_idx: Option<usize>,

    /// カラム詳細のフィルタ文字列
    pub column_filter: String,
    /// 選択中オブジェクトのカラム一覧
    pub columns: Vec<ColumnDetail>,
    /// カラム読込完了後に全項目 SELECT 文を挿入待ちのテーブル名
    pub pending_select_insert_table: Option<String>,

    /// 接続・ログインダイアログ状態
    pub login_dialog: LoginDialogState,
    /// バージョン情報ダイアログ表示フラグ
    pub show_version_dialog: bool,
    /// 表示中の安全確認モーダル
    pub confirm_modal: Option<ConfirmModal>,
    /// 終了確認中フラグ
    pub pending_exit: bool,
    /// 未コミット警告承認後の強制終了許可フラグ
    pub allow_force_exit: bool,
    /// ステータスバーの一時通知メッセージ
    pub status_notice: Option<String>,
}

impl UiState {
    /// 初期状態を構築する（バージョンは `CARGO_PKG_VERSION` から取得）。
    pub fn new(
        config: AppConfig,
        window_state: WindowState,
        profiles: Vec<ConnectionProfile>,
        storage: Option<StoragePaths>,
    ) -> Self {
        let initial_tab = SqlTab::new(
            1,
            "Query1.sql",
            "-- QuQuSQL へようこそ！\n-- Ctrl+Enter または F5 でカーソル位置の SQL を実行します。\n",
        );

        let default_selected = profiles.first().map(|p| p.id.clone());
        let login_dialog = LoginDialogState {
            open: true,
            selected_profile_id: default_selected,
            ..Default::default()
        };

        Self {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            config,
            window_state,
            storage,
            profiles,
            active_profile: None,
            db_info: None,
            read_only_session: true,
            uncommitted_count: 0,
            busy: false,
            tabs: vec![initial_tab],
            active_tab: 0,
            next_tab_id: 2,
            find_replace: FindReplaceState::default(),
            bottom_tab: BottomTab::Log,
            logs: Vec::new(),
            result: None,
            selected_cell: None,
            owners: Vec::new(),
            selected_owner: String::new(),
            selected_object_type: ObjectType::Table,
            object_filter: String::new(),
            objects: Vec::new(),
            selected_object_idx: None,
            column_filter: String::new(),
            columns: Vec::new(),
            pending_select_insert_table: None,
            login_dialog,
            show_version_dialog: false,
            confirm_modal: None,
            pending_exit: false,
            allow_force_exit: false,
            status_notice: None,
        }
    }

    /// 現在アクティブなタブへの不変参照を返す。
    pub fn active_tab_ref(&self) -> Option<&SqlTab> {
        self.tabs.get(self.active_tab)
    }

    /// 現在アクティブなタブへの可変参照を返す。
    pub fn active_tab_mut(&mut self) -> Option<&mut SqlTab> {
        self.tabs.get_mut(self.active_tab)
    }

    /// 新しい空の SQL タブを追加してフォーカスを移す。
    pub fn add_new_tab(&mut self) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = SqlTab::new(id, format!("Query{id}.sql"), "");
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
    }

    /// 指定インデックスのタブを閉じる（最後の 1 タブは閉じない）。
    pub fn close_tab(&mut self, index: usize) {
        if self.tabs.len() <= 1 || index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
    }

    /// アクティブな SQL エディタのカーソル位置へテキストを挿入する。
    pub fn insert_into_active_editor(&mut self, snippet: &str) {
        if let Some(tab) = self.active_tab_mut() {
            let pos = tab.cursor_byte_offset.min(tab.text.len());
            tab.text.insert_str(pos, snippet);
            tab.cursor_byte_offset = pos + snippet.len();
            tab.dirty = true;
        }
    }

    /// 現在接続中のプロファイルが本番環境かどうかを返す。
    pub fn is_production(&self) -> bool {
        self.active_profile
            .as_ref()
            .is_some_and(|p| p.env == EnvironmentTier::Production)
    }

    /// 実行ログエントリを追加する（最大 500 件まで保持）。
    pub fn push_log(
        &mut self,
        sql: String,
        summary: String,
        is_error: bool,
        elapsed_ms: Option<u64>,
    ) {
        let timestamp = chrono::Local::now().format("%Y/%m/%d %H:%M:%S").to_string();
        self.logs.push(QueryLogEntry {
            timestamp,
            sql,
            summary,
            is_error,
            elapsed_ms,
        });
        if self.logs.len() > 500 {
            let drain = self.logs.len() - 500;
            self.logs.drain(0..drain);
        }
    }

    /// 環境区分と接続先・アクティブタブ名を含むウィンドウタイトル文字列を生成する。
    pub fn window_title(&self) -> String {
        let tab_label = self
            .active_tab_ref()
            .map(|t| {
                if t.dirty {
                    format!("{} *", t.title)
                } else {
                    t.title.clone()
                }
            })
            .unwrap_or_else(|| "Query".to_string());

        match &self.active_profile {
            Some(p) => format!(
                "{} {} ({}) - QuQuSQL - [{}]",
                p.env.badge_text(),
                p.name,
                p.display_target(),
                tab_label
            ),
            None => format!("(未接続) - QuQuSQL - [{tab_label}]"),
        }
    }
}
