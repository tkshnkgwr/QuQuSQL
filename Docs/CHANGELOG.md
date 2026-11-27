# QuQuSQL: 変更履歴 (CHANGELOG)

本プロジェクトのすべての主要な変更を記録します。フォーマットは [Keep a Changelog](https://keepachangelog.com/) に準拠し、バージョン番号は [セマンティックバージョニング (SemVer)](https://semver.org/) を採用しています。

---

## [Unreleased]

### Added
- **仕様・設計ドキュメント一式を作成 (`Docs/`)**:
  - `SPECIFICATION.md`: 製品要件・非機能要件（高速起動・省メモリ）・OSqlEdit / OSqlGrid 統合機能仕様・テーブル仕様書出力・オブジェクト DDL 編集・Phase 1〜4 ロードマップを策定。
  - `GUI_DESIGN.md`: 2×2 スプリットペイン構成、タブ設計、編集状態カラーリング、ショートカット一覧を定義。
  - `ARCHITECTURE.md`: Cargo Workspace による 7 クレート構成、Pure Rust Oracle (`oracle-rs`) + SQLite 戦略、省メモリデータ構造・非同期ワーカー設計を策定。
  - `SETTINGS_DESIGN.md`: Windows 標準ディレクトリ構成、`config.toml` / `connections.toml` / `window_state.json` 仕様を定義。
  - `DEVELOPMENT.md`: 開発環境セットアップ、ハイブリッド最適化ビルドプロファイル、クレート境界規約を整備。
  - `TESTING.md`: クレート別テスト戦略・パフォーマンス計測基準を定義。
  - `SECURITY.md`: メモリ安全性・パスワード非保存・本番誤操作防止ポリシーを定義。
  - `TODO.md`: 実装前の全確認項目（Oracle Pure Rust、Oracle+SQLite、`TextEdit` 段階移行、初期スコープ、Windows 優先）を確定記録。
