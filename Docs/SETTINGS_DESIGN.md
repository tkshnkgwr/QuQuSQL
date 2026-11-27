# QuQuSQL: 設定管理・接続プロファイル・テーマ設計書 (SETTINGS_DESIGN)

本書は、`QuQuSQL` における設定ファイルの永続化アーキテクチャ、接続プロファイル管理、テーマ・環境識別カラー、ウィンドウ状態記憶、および設定ダイアログ UI の仕様を定義します。

---

## 1. 設定・キャッシュのディレクトリ構成（Windows 標準準拠）

設定情報（バックアップ・同期対象）とキャッシュ情報（自動生成・再構築可能）を明確に分離し、Windows 標準ディレクトリに配置します（`directories` クレート経由で解決）。

```text
%APPDATA%\QuQuSQL\
├── config.toml                # 基本設定（エディタ・グリッド・文字コード・ショートカット等）
├── connections.toml           # 接続プロファイル一覧（※パスワードは保存しない）
├── themes\                    # カラーテーマ定義
│   ├── light-classic.toml     # デフォルト：OSqlEdit 風の高視認性ライトテーマ
│   └── dark-modern.toml       # ダークテーマ
└── sql_lib\                   # SQL ライブラリ（ユーザー配置の定型 .sql ファイル群）

%LOCALAPPDATA%\QuQuSQL\cache\
├── window_state.json          # ウィンドウ位置・サイズ・最大化・ペイン分割比率
├── query_history.sqlite       # SQL 実行履歴・抽出条件履歴（軽量ローカル DB）
└── filter_history.json        # オブジェクト一覧・詳細のフィルタ入力履歴
```

---

## 2. `config.toml` の設定設計

### Phase 1 実装済み構造（`AppConfig`）
初期フェーズでは軽量・フラットな TOML 構造で保存します。

```toml
default_encoding = "utf-8"              # 新規タブ作成時の文字コード
default_line_ending = "crlf"            # デフォルト改行コード
dark_mode = false                       # false: ライトテーマ / true: ダークテーマ
font_size = 13.5                        # エディタフォントサイズ
default_fetch_limit = 1000              # 初期フェッチ上限行数（1,000行で一旦停止）
date_format = "YYYY-MM-DD HH:MM:SS"     # 日時表示フォーマット（デフォルト ISO 形式）
tns_admin_path = ""                     # tnsnames.ora 手動指定パス（空時は TNS_ADMIN 参照）
confirm_where_less_dml = true           # WHERE 句なし UPDATE / DELETE 実行前に確認
collapse_production_connections = true  # 接続一覧で本番セクションを初期折りたたみ
```

### 将来拡張予定（Phase 2〜3）
機能拡張に合わせて、以下の設定項目（行高・不可視文字表示・ショートカットカスタマイズ等）を順次追加します：
- エディタ関連：`tab_width`, `insert_spaces`, `show_ruler`, `show_invisibles`, `auto_complete`
- グリッド・出力関連：`row_height`, `show_cell_invisibles`, `confirm_before_apply`, `default_spec_format`, `csv_delimiter`
- キーマップ：`[keymap]` セクションでの任意ショートカット再割当

---

## 3. 接続プロファイル設計 (`connections.toml`)

接続先ごとに **環境区分（`env`）** と **環境識別カラー**・**タイトル接頭辞** を持たせることで、「今本番に接続している」ことを視覚的に明確化し、環境区分に応じた確認ダイアログ・動作制約を自動適用します。
なお、初期フェーズの方針に従い、**パスワードはファイルに保存せず接続時に都度入力**します。

```toml
[[connections]]
id = "dev-oracle"
name = "開発環境"
env = "development"                # production / staging / development / local
db_type = "oracle"                 # oracle / sqlite
connect_mode = "direct"            # direct / tns
host = "127.0.0.1"
port = 1521
service_name = "FREEPDB1"
tns_name = ""                      # connect_mode = "tns" 時に使用
username = "USER1"
read_only = false
accent_color = "#2F81F7"           # 青系（開発環境）
confirm_on_commit = false
confirm_on_dml_ddl = false

[[connections]]
id = "stg-oracle"
name = "検証環境"
env = "staging"
db_type = "oracle"
connect_mode = "tns"
tns_name = "STG_DB"
username = "APP_USER"
read_only = false
accent_color = "#BF8700"           # 黄〜オレンジ系（検証環境）
confirm_on_commit = true
confirm_on_dml_ddl = true

[[connections]]
id = "prod-oracle"
name = "本番環境"
env = "production"                 # 本番：赤系警告バナー常時表示＋全確認ガード有効
db_type = "oracle"
connect_mode = "tns"
tns_name = "PROD_DB"
username = "APP_USER"
read_only = true                   # 本番はデフォルト読み取り専用で接続（必要時ロック解除）
accent_color = "#D1242F"           # 赤系（本番環境）
confirm_on_commit = true           # 本番はコミット時確認ダイアログ必須
confirm_on_dml_ddl = true          # 本番は DML / DDL 実行時確認ダイアログ必須

[[connections]]
id = "local-sqlite"
name = "ローカル検証 (SQLite)"
env = "local"
db_type = "sqlite"
file_path = "C:/work/sample.db"
accent_color = "#2DA44E"           # 緑系
```

---

## 4. ウィンドウ・レイアウト状態記憶 (`window_state.json`)

- 実装済み保存項目（`WindowState`）：ウィンドウサイズ（`width`, `height`）、左右分割比率（`left_panel_ratio`）、左上下分割比率（`left_split_ratio`）、右上下分割比率（`right_split_ratio`）、オブジェクトブラウザ表示状態（`show_browser`）。
- 将来拡張予定：ウィンドウ座標 `(x, y)`・最大化フラグおよびマルチモニタ変更時の **画面外脱出ガード**。

---

## 5. 設定ダイアログ UI（egui）

左右 2 ペイン構成（左：カテゴリリスト、右：設定項目フォーム）のモーダルダイアログとして実装します。
- カテゴリ：`全般` / `エディタ` / `グリッド` / `Oracle・接続` / `エクスポート` / `キーマップ`
- 変更時は即座に `egui::Context` のテーマ・フォントサイズへ反映（ライブプレビュー）し、「OK」で `config.toml` へ保存、「キャンセル」でロールバックします。
