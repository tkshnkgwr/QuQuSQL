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

## 2. `config.toml` の主要セクション設計

```toml
[general]
default_encoding = "utf-8"         # 新規ファイル作成時の文字コード（utf-8 / sjis / euc-jp）
default_line_ending = "crlf"       # crlf / lf
theme = "light-classic"
confirm_on_exit_with_uncommitted = true

[editor]
font_family = "BIZ UDGothic"       # Windows システムフォント優先（フォールバックあり）
font_size = 13.0
tab_width = 4
insert_spaces = true
show_line_numbers = true
show_ruler = true
show_invisibles = false            # 半角/全角スペース・タブ・改行記号の可視化
auto_complete = true

[grid]
default_fetch_limit = 1000         # 初期フェッチ件数（1,000行で一旦停止、追加/全件取得可）
date_format = "YYYY-MM-DD HH:MM:SS" # 日時表示フォーマット（デフォルト ISO 形式）
row_height = 22.0                  # 仮想スクロール用の固定行高
show_null_placeholder = "<NULL>"
show_cell_invisibles = false       # セル内改行・タブの可視化
confirm_before_apply = true        # Apply 前に発行 SQL プレビューを表示

[export]
default_spec_format = "xlsx"       # テーブル仕様書出力のデフォルト形式（xlsx / md）
csv_delimiter = ","
csv_quote = "\""

[oracle]
tns_admin_path = ""                # 未指定時は環境変数 TNS_ADMIN を参照
default_auto_commit = false        # 誤操作防止のためデフォルト手動コミット

[keymap]
execute_sql = "F5"
execute_sql_alt = "Ctrl+Enter"
explain_plan = "Ctrl+E"
commit = "Ctrl+Shift+C"
rollback = "Ctrl+Shift+R"
toggle_browser = "F8"
```

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

- 記憶項目：ウィンドウ座標 `(x, y)`、サイズ `(width, height)`、最大化フラグ、左右ペイン分割比率、左上下分割比率、右上下分割比率、オブジェクトブラウザ表示/非表示。
- **画面外脱出ガード**：マルチモニタ構成変更等で保存座標が現在の有効ディスプレイ外になった場合は、プライマリモニタ中央へ自動リセットして起動します。

---

## 5. 設定ダイアログ UI（egui）

左右 2 ペイン構成（左：カテゴリリスト、右：設定項目フォーム）のモーダルダイアログとして実装します。
- カテゴリ：`全般` / `エディタ` / `グリッド` / `Oracle・接続` / `エクスポート` / `キーマップ`
- 変更時は即座に `egui::Context` のテーマ・フォントサイズへ反映（ライブプレビュー）し、「OK」で `config.toml` へ保存、「キャンセル」でロールバックします。
