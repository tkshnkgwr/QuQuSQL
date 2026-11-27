# QuQuSQL: 変更履歴 (CHANGELOG)

本プロジェクトのすべての主要な変更を記録します。フォーマットは [Keep a Changelog](https://keepachangelog.com/) に準拠し、バージョン番号は [セマンティックバージョニング (SemVer)](https://semver.org/) を採用しています。

---

## [Unreleased]

---

## [0.0.5-alpha] - 2026-11-27

### Changed
- **ライセンス変更 (`Cargo.toml`, `LICENSE`, `README.md`)**: プロジェクトのライセンスを `GPL-3.0-or-later`（GNU General Public License v3.0 or later）へ統一し、ルートに `LICENSE` ファイルを追加。
- **プロジェクトルート `README.md` 刷新**: バッジ、開発方針、主な特徴、7 クレート構成の Mermaid アーキテクチャ図・責務表、ビルド／テスト手順、ドキュメント一覧を整備。

---

## [0.0.4-alpha] - 2026-11-27

### Added (Phase 2: テーブル直接編集・UI 強化)
- **直接編集モデル・エクスポート基盤 (`ququ-core`)**:
  - `EditableTableData`, `RowKey`, `GridPatch`（セル編集・行追加/複製/削除マーク・ステップ単位 Undo/Redo・矩形選択統計 `SelectionStats`）を追加。
  - `parse_tsv`（Excel 形式クォート・セル内改行対応）、`export_csv`、`export_csv_with_header`、`export_tsv`、`export_insert_sql`、`format_sql_literal` を実装。
- **最小差分パラメータ化 DML 生成 (`ququ-sql`)**:
  - `generate_grid_dml` により削除（DELETE）→ 変更列のみ最小更新（UPDATE）→ 新規挿入（INSERT）をパラメータ化 SQL（SQLite `?n` / Oracle `:n` + `CHARTOROWID`）および確認モーダル用プレビュー文として生成。
- **SAVEPOINT 保護下のバッチ適用 (`ququ-db`, `ququ-db-sqlite`, `ququ-db-oracle`)**:
  - `DbSession::load_table_for_edit` と `DbSession::apply_dml_batch` を追加し、`SAVEPOINT ququ_grid_apply` と厳密 1 行更新/削除検証（失敗時は全バッチロールバック、自動 `COMMIT` なし）を実装。
  - 読み取り専用セッション時の `ApplyGridDml` ブロックを追加。
- **グリッド編集・範囲コピー・UI/UX 改善 (`ququ-ui`, `ququ-app`)**:
  - オブジェクトブラウザ右クリック「グリッドで開く」、`WHERE` / `ORDER BY` 絞り込み再読込、セルインライン編集（ダブルクリック / `F2`）・`NULL` 設定（`Del`）・行追加/複製/削除・`Undo`/`Redo`・差分セル着色（更新黄・挿入緑・削除赤）、適用前 SQL プレビュー確認モーダル、`Rollback` 後の自動再読込に対応。
  - マウスドラッグ・`Shift+クリック`・行/列ヘッダクリックによる複数セル矩形選択、リアルタイム集計（セル数・数値件数・合計・平均・最小・最大）、選択範囲の TSV / CSV コピー（カラム名あり／なし両対応）、CSV / TSV / INSERT 文ファイル保存を追加。
  - サイドバーのカラム詳細で右クリックによる「カラム名をコピー」「全カラム名をコピー (カンマ区切り)」を追加。
  - ログペインの右クリックメニュー（「ログのコピー」「ログのクリア」）を追加し、ペイン全体（右側余白含む）で右クリックが反応するよう改善。
  - Zed 風の SQL タブバー（左右横移動ボタン `←`/`→`・横スクロール・各タブの `×` 閉じるボタン）と、アプリ終了時の未保存 SQL タブ警告を追加し、豆腐文字（`✕`）を解消。

---

## [0.0.3-alpha] - 2026-11-27

### Fixed / Improved
- **SQLite カラム長 (`len`) 表示対応 (`ququ-db-sqlite`)**: `PRAGMA table_info` の型文字列（例：`char(20)`、`number(10, 2)`）から型名と長さ部分を分離し、詳細ペインの `len` 列へ表示するよう改善（単体テスト追加）。
- **結果グリッドの数値右寄せ＆セル全体クリック対応 (`ququ-ui`)**: 数値型（`Int` / `Float` / `Decimal`）および行番号 `#` 列を右寄せ描画し、セル矩形全体でクリック選択・右クリックメニューが反応するよう改善。
- **サイドバー行全体選択・ホバー改善 (`ququ-ui`, `ququ-app`)**: オブジェクト一覧・カラム詳細の各セルで文字の後ろの余白を含めてクリック選択・ダブルクリック挿入・右クリック・指アイコンホバーが効くよう改善し、カラム詳細の選択行ハイライトを追加。

---

## [0.0.2-alpha] - 2026-11-27

### Added
- **メニュー「ヘルプ(H)」→「バージョン情報...」**（バージョン番号は `CARGO_PKG_VERSION` を参照しソース内ハードコーディングを排除）。
- **DDL 実行時のサイドバー自動更新**（サイドバー表示中に `CREATE TABLE` 等の DDL を実行するとオブジェクト一覧を自動再読込）。
- **オブジェクト一覧右クリックメニュー拡充**:
  - 「DDL を取得」（取得した DDL をアクティブな SQL エディタへ貼り付け）。
  - 「SELECT 文を挿入 (全項目)」（対象テーブルの全カラムを展開した整形済み `SELECT` 文を挿入）。
- **テスト・ドキュメント拡充**:
  - 全 24 本の Rust ソースファイルへ日本語ドキュメントコメント（`//!` / `///`）および処理コメントを整備。
  - 全 6 ライブラリクレートへ `#![warn(missing_docs)]` を適用し、`ququ-core` / `ququ-sql` に Doctest を追加。
  - `ququ-core` / `ququ-sql` / `ququ-db`（モックワーカー結合テスト）/ `ququ-db-sqlite` / `ququ-db-oracle` / `ququ-ui` の単体・結合テストを拡充。
  - `Docs/` 配下の仕様書・設計書を現在の実装内容へ同期最新化。

---

## [0.0.1-alpha] - 2026-11-27

### Added
- **Phase 1 MVP（7 クレート構成のマルチクレート Workspace と基盤実装）**:
  - **マルチクレート骨格 (`Cargo.toml`)**: `ququ-core`, `ququ-sql`, `ququ-db`, `ququ-db-sqlite`, `ququ-db-oracle`, `ququ-ui`, `ququ-app` の 7 クレート構成とハイブリッド最適化プロファイルを整備。
  - **省メモリコア＆設定 (`ququ-core`)**: 1 次元フラット配列 `ResultSet`、`Decimal(CompactString)` による高精度 `NUMBER` 保持、パスワード非保存・メモリゼロクリア付き接続プロファイル、環境区分（`Local` / `Development` / `Staging` / `Production`）、`tnsnames.ora` パーサを実装。
  - **SQL 解析・安全判定 (`ququ-sql`)**: SQL トークナイザ、セミコロン `;` 区切りの文分割＆カーソル位置抽出、`StatementKind` 判定、`WHERE` 句なし `UPDATE` / `DELETE` 検知を実装。
  - **非同期 DB ワーカー＆ドライバ (`ququ-db`, `ququ-db-sqlite`, `ququ-db-oracle`)**: UI 非ブロッキングの専用 DB ワーカー、SQLite 手動トランザクション（`BEGIN TRANSACTION` / 明示 `COMMIT` / `ROLLBACK`）、Pure Rust Oracle ドライバ（`oracle-rs`、ISO 日時・`NUMBER` 厳密保持・カーソル追加フェッチ・メタデータ取得）を実装。
  - **GUI＆誤操作防止 UI (`ququ-ui`, `ququ-app`)**: `eframe` (`glow`) + `mimalloc` + Windows 日本語フォント読込による 2×2 スプリットペイン、行番号・SQL ハイライト・検索＆置換バー、仮想スクロール結果グリッド、環境別セクション分離ログイン（本番赤枠折りたたみ＋接続ボタン警告化）、本番接続時の赤系常時警告バナー＋Read-Only 初期接続、キャンセル固定の安全確認モーダルを実装。

### Fixed
- 2×2 スプリットペイン間のウィジェット ID 衝突を各ペイン固有の `id_salt` で解消 (`ququ-ui`)。
- サイドバーのオブジェクト一覧・カラム詳細ホバー時のカーソルを指アイコン（ポインター）に変更 (`ququ-ui`)。

---

### Added (Docs)
- **仕様・設計ドキュメント一式を作成 (`Docs/`)**:
  - `SPECIFICATION.md`: 製品要件・非機能要件（高速起動・省メモリ）・OSqlEdit / OSqlGrid 統合機能仕様・テーブル仕様書出力・オブジェクト DDL 編集・Phase 1〜4 ロードマップを策定。
  - `GUI_DESIGN.md`: 2×2 スプリットペイン構成、タブ設計、編集状態カラーリング、ショートカット一覧を定義。
  - `ARCHITECTURE.md`: Cargo Workspace による 7 クレート構成、Pure Rust Oracle (`oracle-rs`) + SQLite 戦略、省メモリデータ構造・非同期ワーカー設計を策定。
  - `SETTINGS_DESIGN.md`: Windows 標準ディレクトリ構成、`config.toml` / `connections.toml` / `window_state.json` 仕様を定義。
  - `DEVELOPMENT.md`: 開発環境セットアップ、ハイブリッド最適化ビルドプロファイル、クレート境界規約を整備。
  - `TESTING.md`: クレート別テスト戦略・パフォーマンス計測基準を定義。
  - `SECURITY.md`: メモリ安全性・パスワード非保存・本番誤操作防止ポリシーを定義。
  - `TODO.md`: 実装前の全確認項目（Oracle Pure Rust、Oracle+SQLite、`TextEdit` 段階移行、初期スコープ、Windows 優先）を確定記録。
