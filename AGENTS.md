# Addness CLI — Codex ガイドライン

## ビルド・チェックコマンド
- `cargo build` — ビルド
- `cargo test` — テスト実行
- `cargo clippy` — lint（warningをゼロに保つこと）
- `cargo fmt -- --check` — フォーマットチェック

## Rustコーディングルール
- **clippy準拠必須**: コード変更後は `cargo clippy` を実行し、warningをすべて解消する
- **format!のインライン変数**: `format!("{}", var)` ではなく `format!("{var}")` を使う（変数が1トークンの場合）
- **生文字列記法**: 複数行の文字列やエスケープが多い場合は `r#"..."#` を使う
- **完全一致比較**: パス比較等では `starts_with()` ではなく `==` で完全一致させる

## Git運用
- **PRのベースブランチは `main`**（`master` ではない）。`gh pr create` で `--base main` を指定する

## Addness連携 — Addnessをタスク DB として使う

このリポジトリで作業するとき、**Addness があなたの真実源（タスク DB）**です。
目標・進捗・決定・学びは、ローカルのメモではなく Addness のゴール／コメントに読み書きしてください。
読み書きは同梱の `addness` CLI を通じて行います（MCP は不要）。

### 基本コマンド
- `addness skills` — 全コマンドの使い方を出力（迷ったらまずこれ）
- データ取得系は `--json` を付けると構造化出力になる（例: `addness goal get <ID> --json`）
- `addness detect-goal` — 現在の git ブランチ名から対象ゴール ID を推定

### 作業の進め方（理想 − 現在 = アクション）
Addness のゴールは「理想の状態（DoD = 完了基準）」と「現在の状態（説明）」のギャップを、
子ゴール（アクション）に分解して埋める仕組みです。これに沿って動いてください。

1. **DoD の確認と具体化** — 対象ゴールの DoD（完了基準）を `addness goal get <ID> --json` で確認する。
   曖昧・不十分なら**ユーザーと対話して具体化**し、`addness goal update <ID> ...` で書き戻す。
   勝手に決めず、不明点は質問してから固める。
2. **差分の分解** — 理想と現在の差分を埋めるアクションは、子ゴールとして
   `addness goal create ...`（親に対象ゴールを指定）で分解する。
3. **進捗・決定の記録** — 実装の決定や進捗は `addness comment create --goal <ID> --body "..."` に残す。
   コメント末尾に **`Codexより`** と署名し、人間のコメントと区別する。
   コメントは「進捗ログ」ではなく、理想実現に必要なコンテキストを集める場であることを意識する。
4. **成果物・PR** — 生成した PR やファイルは `addness link` / `addness deliverable` でゴールに紐づける。

### 注意
- ステータス `CANCELLED` は「中止」ではなく「一時停止」を意味する。親が CANCELLED でも配下を勝手に動かさない。
- ゴールはタイトル名で呼ぶ（ID は補助情報）。
