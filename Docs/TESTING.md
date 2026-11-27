# QuQuSQL: テスト方針・計画 (TESTING)

本書は、`QuQuSQL` におけるテスト戦略、クレート別の単体テスト・結合テスト、およびパフォーマンス・省メモリ検証基準を定義します。

---

## 1. テスト戦略

GUI（`egui`）を起動せずとも、コアデータ構造・SQL 解析・ワーカー制御・SQLite 実行・UI ユーティリティのすべてをヘッドレスで高速に自動テストできる構成とします。

### クレート別テスト領域

| クレート | テスト種別 | 主な検証項目 |
| :--- | :--- | :--- |
| **`ququ-core`** | 単体＋Doctest | `ResultSet` のフラット配列アクセス＆結合、`CellValue`（`Decimal` 高精度保持・`write_display_into`・`Bytes`）、`ObjectType::from_db_str`、`EnvironmentTier` プリセット、`ConnectionProfile`（パスワード非シリアライズ・`display_target`）、`tnsnames.ora` パーサ |
| **`ququ-sql`** | 単体＋Doctest | トークナイザ（バインド変数・エスケープ文字列・日本語識別子）、SQL 文分割（文字列・コメント内の `;` 無視・末尾セミコロン省略）、カーソル位置抽出、`StatementKind` 判定、`WHERE` 句なし `UPDATE` / `DELETE` 検知 |
| **`ququ-db`** | 結合（モック） | モック `DbSession` による `DbWorker` 非同期テスト（`allow_write = false` 時の DML/DDL/COMMIT 拒否、DDL 時の `is_ddl = true` 返却、`LoadDdl` 連携） |
| **`ququ-db-sqlite`** | 結合テスト | インメモリ SQLite に対するクエリ実行、1,000 行フェッチ上限＆追加取得、メタデータ取得、DDL 取得（`sqlite_master`）、手動トランザクション（Commit / Rollback 復元）、全データ型ラウンドトリップ |
| **`ququ-db-oracle`** | 単体＋実機統合 | `oracle-rs` Direct / `tnsnames.ora` 設定解決とバリデーション、Oracle `Value` 変換（25 桁 `NUMBER` 厳密保持、ISO 日時、`Null`）。実機接続テストは `#[ignore]` 付きで環境変数指定時のみ実行 |
| **`ququ-ui`** | 単体テスト | 検索＆置換ロジック（大文字小文字区別・正規表現・日本語オフセット行列変換）、TSV エクスポート（Excel 互換エスケープ・NULL 表現）、全項目 SELECT 文フォーマット、`parse_hex_color` フォールバック |

---

## 2. テスト＆ドキュメント検証コマンド

```powershell
# 1. ワークスペース全体の通常テスト＋Doctest（外部 DB 不要で完結）
cargo test --workspace --all-targets
cargo test --workspace --doc

# 2. 特定クレートのテスト
cargo test -p ququ-core
cargo test -p ququ-sql
cargo test -p ququ-db
cargo test -p ququ-db-sqlite
cargo test -p ququ-ui

# 3. Oracle 実機統合テスト（接続情報を環境変数で指定時のみ）
cargo test -p ququ-db-oracle -- --ignored --nocapture

# 4. Clippy 静的解析・RustDoc ビルド検証（missing_docs 警告なし確認）
cargo clippy --workspace --all-targets
cargo doc --workspace --no-deps
```

---

## 3. パフォーマンス・省メモリ検証基準

各フェーズ完了時に `--release` ビルドで以下を計測し、非機能要件（[SPECIFICATION.md](./SPECIFICATION.md)）を満たしていることを確認します。

1. **コールドスタート時間**: 起動からメインウィンドウ初回描画まで **500 ms 以内**（目標 200 ms）。
2. **アイドル時リソース**: タスクマネージャ上で **メモリ 30〜50 MB 以下**、**CPU 使用率 0%**。
3. **大量データベンチマーク**: SQLite インメモリで **10 万行 × 20 列** を生成・取得し、スクロール 60 FPS 維持および **メモリ 150 MB 以下** を確認。
4. **テーブル仕様書一括出力**: 50 テーブル選択時の `.xlsx` / `.md` 生成が数秒以内に完了することを確認。
