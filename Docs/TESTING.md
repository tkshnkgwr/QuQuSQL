# QuQuSQL: テスト方針・計画 (TESTING)

本書は、`QuQuSQL` におけるテスト戦略、クレート別の単体テスト・結合テスト、およびパフォーマンス・省メモリ検証基準を定義します。

---

## 1. テスト戦略

GUI（`egui`）を起動せずとも、コアデータ構造・SQL 解析・差分 DML 生成・エクスポート・SQLite 実行のすべてをヘッドレスで高速に自動テストできる構成とします。

### クレート別テスト領域

| クレート | テスト種別 | 主な検証項目 |
| :--- | :--- | :--- |
| **`ququ-core`** | 単体テスト | `ResultSet` のフラット配列アクセス、`CellValue` / `CompactString` のメモリ効率、`GridPatch`（追加・変更・削除・リバート）、文字コード自動判別（UTF-8 / SJIS / EUC-JP）、CSV / TSV（セル内改行含む）パース＆出力、`tnsnames.ora` パーサ、テーブル仕様書（Excel / Markdown）生成 |
| **`ququ-sql`** | 単体テスト | SQL 文分割（文字列・コメント内の `;` 無視）、カーソル位置の 1 文抽出、バインド変数抽出（`:var`）、文脈補完（エイリアス解決）、変更カラムのみの最小 `UPDATE` / `INSERT` / `DELETE` 生成、SQL フォーマッタ |
| **`ququ-db-sqlite`** | 結合テスト | インメモリ SQLite に対するクエリ実行、メタデータ取得、DDL 取得（`sqlite_master`）、トランザクション（Commit / Rollback）、差分適用の一貫性 |
| **`ququ-db-oracle`** | 単体＋実機統合 | `oracle-rs` 接続設定変換、Oracle 向けディクショナリ SQL 生成、DDL フォールバック再構築。実機接続テストは `#[ignore]` 付きで環境変数指定時のみ実行 |
| **`ququ-ui`** | 単体テスト | 仮想スクロールの可視行インデックス計算、グリッド選択範囲の集計（件数・合計・平均・最小・最大）、フィルタ/ソートのインデックス写像 |

---

## 2. テスト実行コマンド

```powershell
# 1. ワークスペース全体の通常テスト（外部 DB 不要で完結）
cargo test --workspace --all-targets

# 2. 特定クレートのテスト
cargo test -p ququ-core
cargo test -p ququ-sql
cargo test -p ququ-db-sqlite

# 3. Oracle 実機統合テスト（接続情報を環境変数で指定時のみ）
cargo test -p ququ-db-oracle -- --ignored --nocapture
```

---

## 3. パフォーマンス・省メモリ検証基準

各フェーズ完了時に `--release` ビルドで以下を計測し、非機能要件（[SPECIFICATION.md](./SPECIFICATION.md)）を満たしていることを確認します。

1. **コールドスタート時間**: 起動からメインウィンドウ初回描画まで **500 ms 以内**（目標 200 ms）。
2. **アイドル時リソース**: タスクマネージャ上で **メモリ 30〜50 MB 以下**、**CPU 使用率 0%**。
3. **大量データベンチマーク**: SQLite インメモリで **10 万行 × 20 列** を生成・取得し、スクロール 60 FPS 維持および **メモリ 150 MB 以下** を確認。
4. **テーブル仕様書一括出力**: 50 テーブル選択時の `.xlsx` / `.md` 生成が数秒以内に完了することを確認。
