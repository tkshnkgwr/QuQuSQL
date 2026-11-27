# QuQuSQL: 開発者ガイド (DEVELOPMENT)

本書は、`QuQuSQL` のローカル開発環境のセットアップ、マルチクレートワークスペースのビルド・実行コマンド、ハイブリッド最適化ビルドプロファイル、および開発ルールについて説明します。

---

## 1. 開発環境のセットアップ

### 前提条件
- **OS**: Windows 10 / 11 (x86_64)
- **Rust ツールチェーン**: Rust 1.85 以上（最新 stable 推奨）
- **Git**: バージョン管理
- **Oracle Instant Client**: **不要**（Pure Rust ドライバ `oracle-rs` を採用しているため外部 DLL 依存なし）

```powershell
# ツールチェーンの確認
rustc --version
cargo --version
```

### リポジトリのクローン
```powershell
git clone https://github.com/tkshnkgwr/QuQuSQL.git
cd QuQuSQL
```

---

## 2. コマンドクイックリファレンス

| 用途 | 実行コマンド | 特徴・動作速度 |
| :--- | :--- | :--- |
| **日常の開発・デバッグ起動** | `cargo run -p ququ-app` | 自前コードは高速インクリメンタルビルド、外部 GUI 依存はフル最適化で 60 FPS 維持 |
| **リリース最適化での動作確認** | `cargo run -p ququ-app --release` | LTO・最大最適化を適用した本番相当の起動速度・メモリ消費を検証 |
| **配布用単一 EXE のビルド** | `cargo build -p ququ-app --release` | `target/release/ququ-app.exe`（単一バイナリ）を出力 |
| **全クレートの型・警告チェック** | `cargo clippy --workspace --all-targets` | 未使用コード・非効率なアロケーション等を検出 |
| **フォーマット統一** | `cargo fmt --all` | プロジェクト全体のコードスタイルを統一 |

---

## 3. ハイブリッド最適化ビルドプロファイル

`egui` / `eframe` 開発では、通常の Debug ビルドだと描画処理や仮想スクロールが極端に重くなります。開発時の高速ビルドと滑らかな GUI 動作を両立するため、ルート `Cargo.toml` にハイブリッド最適化プロファイルを採用します。

```toml
# ① 外部依存クレート（egui, epaint, wgpu/glow, oracle-rs 等）は開発時もフル最適化
[profile.dev.package."*"]
opt-level = 3

# ② 自前ワークスペースクレートは軽量最適化でインクリメンタルビルドを高速維持
[profile.dev]
opt-level = 1

# ③ 配布用リリースビルドの極限最適化（速度重視・バイナリ軽量化）
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
strip = true
```

---

## 4. クレート境界・コーディング規約

1. **UI と DB 通信の完全分離**:
   - `ququ-ui` から `ququ-db` への直接依存を禁止する。すべての DB 操作はコマンド経由で非同期ワーカーへ委譲する。
2. **描画ループ内でのヒープ確保禁止**:
   - `ququ-ui` の仮想グリッドやエディタの毎フレーム描画処理内で、不要な `String` / `Vec` の新規割り当てを行わない。
3. **非同期・キャンセル安全性**:
   - 長時間実行されうるクエリやメタデータ取得は必ずキャンセル可能にし、UI スレッドを絶対にブロックしない。
4. **ドキュメント同期**:
   - 機能追加・仕様変更時は `Docs/` 配下の関連仕様書および `Docs/CHANGELOG.md` を合わせて更新する。
