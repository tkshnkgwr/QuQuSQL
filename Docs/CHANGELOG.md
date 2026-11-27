# QuQuSQL: 変更履歴 (CHANGELOG)

本プロジェクトのすべての主要な変更を記録します。フォーマットは [Keep a Changelog](https://keepachangelog.com/) に準拠し、バージョン番号は [セマンティックバージョニング (SemVer)](https://semver.org/) を採用しています。

---

## [Unreleased]

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
