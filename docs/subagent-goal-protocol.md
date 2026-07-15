# サブエージェント=子ゴール乗っ取り運用プロトコル

メインAI（設計・監査担当）がAddnessの子ゴールを作成し、サブエージェント（実装担当）が
その子ゴールを「乗っ取って」実装・検証・PR作成・ログ記録までを一貫して行う運用手順。
本ドキュメントは、実際に子ゴール単位でサブエージェントへ実装を委譲して運用してきた
手順を再現可能な形にまとめたものである（例: 通知既読CLI追加、棚卸し表docs整備、
FileChange可視化）。

## 1. 役割分担

| 役割 | 担当 | 内容 |
|---|---|---|
| メインAI | Fable（メイン会話） | 設計、子ゴール作成（DoD定義）、進捗監査、レビュー、完了処理 |
| サブエージェント | Sonnet（委譲先） | 担当子ゴールの実装・検証・PR作成・ゴールへのログ記録 |

メインAIは実装そのものには極力手を出さず、「何を・どういう完了基準で作るか」を子ゴールに
落とし込むこと、複数のサブエージェントの成果を監査してから閉じることに専念する。

Opusは使わない。実装委譲はSonnet、メインAIの設計・監査はFable（メイン、実装委譲はSonnet）で行う。

## 2. 子ゴール作成（メインAI）

```bash
addness goal create \
  --title "<作業名>" \
  --parent <親GOAL_ID> \
  --description "<DoD: 検証コマンドとグリーン基準を含む完了条件>" \
  --json
```

`--description`（DoD）には、サブエージェントが自己完結で「終わったか」を判定できるよう、
**検証コマンドと合格基準**を必ず含める。例:

```text
`cargo build && cargo clippy && cargo test` がすべて成功し、
`addness goal list --json` に新設フィルタが出力されること。
```

DoDが曖昧な場合、サブエージェントは実装を始める前にメインAI（またはゴールオーナー）に
コメントで確認してから着手する。

## 3. 乗っ取りプロトコル（サブエージェントへの指示テンプレート）

サブエージェントに子ゴールを渡すときは、以下をそのまま指示文に含める。

### 3.1 開始時ログ

作業に着手する前に、方針をゴールへコメントする（進捗の垂れ流しではなく、
「何をどう実装するか」の方針表明として1回）。

```bash
addness comment create --goal <GOAL_ID> \
  --body "<方針の要約>（サブエージェント実装ログ）Claude Codeより" \
  --json
```

### 3.2 ブランチ作成

```bash
git fetch origin main
git checkout -b <branch-name> origin/main
```

- 配線ファイル（`src/main.rs`、`Cargo.toml` 等）が先行PRと重なる場合は、
  `origin/main` からではなく **その先行ブランチからスタック** し、
  `gh pr create --base <先行ブランチ>` でベースを先行ブランチに向ける
  （直列化。詳細は §6）。

### 3.3 実装・検証

DoDの検証コマンド（`cargo build` / `cargo clippy` / `cargo test` / `cargo fmt -- --check` など）
をすべてグリーンにしてからPRを作成する。

### 3.4 完了時ログ・PR作成・リンク

```bash
gh pr create --base main \
  --title "<PRタイトル>" \
  --body "$(cat <<'EOF'
## Summary
- ...

## Test plan
- [ ] cargo build
- [ ] cargo clippy
- [ ] cargo test

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"

addness link pr --goal <GOAL_ID> --url <PR_URL> --name "<成果物名>" --json

addness comment create --goal <GOAL_ID> \
  --body "完了: <結果の要約> PR: <PR_URL>（サブエージェント実装ログ）Claude Codeより" \
  --json
```

### 3.5 addnessコマンド失敗時の扱い

`addness` コマンド（ネットワーク不調・認証切れ等）が失敗しても、**実装作業は止めない**。
実装・PR作成を優先して進め、addnessへの記録はセッションの最後にまとめて再試行する。

## 4. 完了処理（メインAI）

サブエージェントからPR URLの報告を受けたら、メインAIが以下を確認してから子ゴールを閉じる。

1. PRのdiffが子ゴールのDoDと一致しているか（余計な変更が混ざっていないか）を監査する。
2. CI（`cargo build` / `cargo clippy` / `cargo test` / `cargo fmt -- --check`）がグリーンか確認する。
3. 問題なければ子ゴールを完了にする。

```bash
addness goal update <GOAL_ID> --status COMPLETED
```

差し戻しが必要な場合は、子ゴールへコメントで具体的な修正指示を残し、同じサブエージェントに
再委譲する（新規ゴールを作らない）。

## 5. モデル方針

- 実装を担当するサブエージェント: **Sonnet**
- 設計・子ゴール分解・監査・レビューを行うメインAI: **Fable**（メイン、実装委譲はSonnet）
- **Opusは使わない**

## 6. 並列化の指針

- 触るファイルが重ならない子ゴール同士は **並列実行してよい**。
- `src/main.rs`・`Cargo.toml`・`AGENTS.md` などの配線ファイルを複数の子ゴールが同時に
  触れる可能性がある場合（例: CLIサブコマンドの追加が複数走る場合）は、
  **スタックPRで直列化**する。後発の子ゴールは先行ブランチから分岐し、
  PRのbaseも先行ブランチに向けてマージ順の衝突を避ける。
- 純粋なdocs追加やロジックが独立したファイルの新設は並列化の good candidate。

## 実例

この運用は以下の子ゴールで実施済み:

- 通知既読CLI（`addness notification` 系サブコマンド追加）
- 棚卸し表docs整備（`docs/cli-write-coverage-plan.md` 系）
- FileChange可視化（TUI側の差分表示機能）

いずれも「メインAIが子ゴールを作成 → サブエージェントが乗っ取り、開始コメント →
実装・検証 → PR作成・ゴールへのリンク → 完了コメント」という同一の流れで進めている。
