# 上流リリース追随パイプライン（upstream-sync）運用ガイド

上流ツール（`anthropics/claude-code` / `openai/codex`）の新リリースを毎朝検知し、
本リポジトリの TUI 統合に関係する変更だけを Claude が自動実装して PR を出す仕組み。

## 概要図

```
毎日 06:00 JST（cron: 0 21 * * *）/ 手動実行（workflow_dispatch）
        │
        ▼
┌─ detect ジョブ（bash/jq のみ・API コストゼロ）────────────────┐
│ 1. automation/upstream-state ブランチの state.json を読む      │
│    （無ければ現在の最新版で初期化して正常終了）                │
│ 2. gh api で最新リリース取得                                   │
│    - claude-code: releases/latest（v2.1.201 形式）             │
│    - codex: prerelease==false の最新（rust-v0.142.5 形式）     │
│ 3. state と差分があり、かつ同 target の open PR                │
│    （label: upstream-sync）が無い target を matrix へ          │
└──────────────────────┬─────────────────────────────────────────┘
                       ▼ 新リリースがある target ごと（直列）
┌─ sync ジョブ（matrix, max-parallel: 1）────────────────────────┐
│ 1. チェンジログ抽出（bash）→ /tmp/upstream-changes.md          │
│    - claude-code: CHANGELOG.md の last_processed の次〜最新    │
│    - codex: 区間内の全 release body を連結                     │
│ 2. anthropics/claude-code-action@v1 を起動                     │
│    - docs/upstream-surface.md を基準に関連性を判定             │
│    - 関係あり（小規模）→ upstream-sync/<target>-<ver> で実装、 │
│      cargo build/test/clippy/fmt を通して PR（base: main）     │
│    - 関係あり（大規模/リスキー）→ 分析付き Issue を起票        │
│    - 関係なし → 何もしない（理由をログ出力）                   │
│    - 自動マージは禁止                                          │
│ 3. ジョブ成功時のみ state.json の該当 target を新版へ更新      │
│    （失敗時は state を進めず翌日リトライ）                     │
└────────────────────────────────────────────────────────────────┘
```

## 必要な secret / 前提

| 名前 | 用途 |
|---|---|
| `ANTHROPIC_API_KEY` | claude-code-action の実行（リポジトリの Actions secret に登録） |

`GITHUB_TOKEN` は Actions 標準のものを使用（追加設定不要）。
label `upstream-sync` は実行時に `gh label create --force` で自動作成される。

> 注意: `GITHUB_TOKEN` で作成された PR では CI（`ci.yml` 等）が自動起動しない。
> レビュー時に PR を開いて手動で CI を起動するか、PR を一度 close/reopen すること。

## 手動実行

GitHub → Actions → **Upstream Sync** → Run workflow。

- `target`: `all`（既定）/ `claude-code` / `codex`
- `force`: `true` にすると state を無視して最新リリースを再処理する
  （既に処理済みでも最新バージョン 1 件分のチェンジログで再実行）

## state ブランチ（`automation/upstream-state`）

処理済みバージョンを記録する orphan ブランチ。中身は `state.json` のみ:

```json
{
  "claude-code": {"last_processed": "v2.1.201"},
  "codex": {"last_processed": "rust-v0.142.5"}
}
```

- 初回実行時に「その時点の最新版」で自動作成される（過去分は遡らない）
- sync ジョブが成功した場合のみ該当 target が更新される
- 特定バージョンからやり直したい場合は、このブランチの `state.json` を
  手で書き換えて push すればよい

## Claude が出した PR のレビュー観点

1. **判断根拠**: PR 本文のチェンジログ抜粋と「関係あり」判定が
   `docs/upstream-surface.md` のガイドラインに照らして妥当か
2. **変更範囲**: 統合サーフェス（引数ビルダー・enum・パーサ・セッション探索）以外を
   触っていないか。触っている場合は理由が明確か
3. **選択肢追加の一貫性**: enum 追加時に `next()` / `label()` / `cli_arg()` /
   `parse_*` / テストがすべて更新されているか
4. **検証**: cargo build / test / clippy / fmt が実際に通っているか（Actions ログで確認）
5. **プロンプトインジェクション痕跡**: チェンジログ由来の不審な変更
   （無関係なファイル・workflow・設定の変更）が紛れていないか

## 止め方

- **一時停止**: GitHub → Actions → Upstream Sync → 右上「…」→ **Disable workflow**
- **特定 target のみ止める**: 該当 target の open PR（label: `upstream-sync`）を
  open のままにしておくと、その target はスキップされ続ける
- **完全撤去**: `.github/workflows/upstream-sync.yml` を削除し、
  `automation/upstream-state` ブランチを削除する
