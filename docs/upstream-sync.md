# 上流リリース追随パイプライン（upstream-sync）運用ガイド

上流ツール（`anthropics/claude-code` / `openai/codex`）の新リリースを毎朝検知し、
本リポジトリの TUI 統合に関係する変更だけを Codex が自動実装して PR を出す仕組み。

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
│ 1.5 プローブ検証（新バージョン実バイナリ）→ /tmp/probe-report.md│
│    - npm で新バージョン CLI を導入し #[ignore] プローブを実行   │
│    - 失敗してもワークフローは落とさず Codex の判定材料にする    │
│ 2. codex exec（@openai/codex）を非対話実行                     │
│    - docs/upstream-surface.md を基準に関連性を判定             │
│    - 関係あり（小規模）→ upstream-sync/<target>-<ver> で実装、 │
│      cargo build/test/clippy/fmt を通して PR（base: main）     │
│    - 関係あり（大規模/リスキー）→ 分析付き Issue を起票        │
│    - 関係なし → 何もしない（理由をログ出力）                   │
│    - 自動マージは禁止                                          │
│ 3. ジョブ成功時のみ state.json の該当 target を新版へ更新      │
│    （失敗時は state を進めず翌日リトライ）                     │
│ 4. Codex が PR / Issue を作成していれば Addness ゴールを作成し  │
│    対応先 URL をリンク（ADDNESS_API_TOKEN 未設定ならスキップ・ │
│    失敗しても sync 本体は落とさない）                          │
└────────────────────────────────────────────────────────────────┘
```

## 必要な secret / 前提

| 名前 | 種別 | 用途 |
|---|---|---|
| `OPENAI_API_KEY` | secret | codex CLI（`codex exec`）の実行。リポジトリの Actions secret に登録済み。ワークフロー内では env 経由でのみ参照し、`codex login --with-api-key` に stdin で渡す |
| `ADDNESS_API_TOKEN` | secret（任意） | Addness ゴール作成ステップの認証トークン。未設定ならゴール作成はスキップされる（sync 本体には影響しない） |
| `ADDNESS_ORG_ID` | variable（任意） | ゴールを作成する組織 ID。設定時は `addness goal create --org` に渡され、`link pr` の組織スコープにも使われる |
| `ADDNESS_PARENT_GOAL_ID` | variable（任意） | 作成するゴールの親ゴール ID。設定時は `addness goal create --parent` に渡される（未設定ならルートゴールとして作成） |

`GITHUB_TOKEN` は Actions 標準のものを使用（追加設定不要）。
label `upstream-sync` は実行時に `gh label create --force` で自動作成される。

### Addness ゴール作成ステップ

sync ジョブの最後（state 更新の後）に、Codex が作成した追随対応を Addness の
ゴールとして起票し追跡する **Addness ゴール作成** ステップがある。

- **スキップ条件**: secret `ADDNESS_API_TOKEN` が未設定なら `::notice::` を出して
  何もせず終了する。ゴール作成の失敗（ビルド・API エラー等）でも `continue-on-error`
  により sync ジョブ自体は落とさない。
- **対応先の特定**: `upstream-sync/<target>-<新バージョン>` の open PR を優先して探し、
  無ければ label `upstream-sync` の open Issue をタイトルの新バージョンで検索する。
  どちらも無ければ「作成不要」として終了する。
- **ゴール作成**: `cargo build --release --locked` で CLI をビルドし、
  環境変数トークン認証（`ADDNESS_API_TOKEN` / 任意で `ADDNESS_ORG_ID`）で
  `addness goal create --title "[upstream-sync] <target> <ver> 追随" --description ... --json`
  を実行。`ADDNESS_ORG_ID` があれば `--org`、`ADDNESS_PARENT_GOAL_ID` があれば `--parent`
  を付ける。作成後、レスポンスの `id` に対して `addness link pr` で対応先 URL をリンクする。

**セットアップ手順**（ゴール作成を有効化する場合）:

1. リポジトリの発行済み API キー（個人 API キー等。`addness api-key list` で確認）を
   secret に登録する: GitHub → Settings → Secrets and variables → Actions →
   **Secrets** タブ → New repository secret → 名前 `ADDNESS_API_TOKEN`。
2. 組織を固定したい場合は **Variables** タブに `ADDNESS_ORG_ID` を追加する（任意）。
3. 追随ゴールを特定の親ゴール配下にまとめたい場合は **Variables** タブに
   `ADDNESS_PARENT_GOAL_ID` を追加する（任意。未設定ならルートゴール）。
4. `ADDNESS_API_TOKEN` を登録しなければゴール作成ステップは静かにスキップされるため、
   この機能を使わない運用も可能。

> 注意: `GITHUB_TOKEN` で作成された PR では CI（`ci.yml` 等）が自動起動しない。
> レビュー時に PR を開いて手動で CI を起動するか、PR を一度 close/reopen すること。

## プローブ検証（統合サーフェスの実測）

チェンジログの静的分析だけでは、明記されない挙動変更を捕まえられない
（例: codex 0.142.5 の app-server が JSON-RPC メッセージから `"jsonrpc":"2.0"` を
削り TUI がフリーズしたケース）。これを防ぐため、sync ジョブはチェンジログ抽出の直後に
**新バージョンの実バイナリを CI に導入し、統合前提を実測検証するプローブ**を走らせる。

**何を検証するか**（`#[ignore]` 付き in-crate テスト。名前は `upstream_probe_` プレフィックス）:

- `upstream_probe_codex_appserver_handshake` — 実 `codex app-server` を spawn し、
  `initialize` を送って応答が JSON-RPC の `Response { id: 1 }` としてパースできるか
  （`jsonrpc` フィールド欠落を含む実プロトコルを実測）
- `upstream_probe_codex_cli_flags` — `codex exec --help` / `codex --help` に、
  本リポジトリがワンショット/常駐で渡すサブコマンド・フラグが存在するか
- `upstream_probe_claude_cli_flags` — `claude --help` に、`resident_args` / `exec_args`
  が渡すフラグが存在するか

**失敗時の扱い**: プローブが FAIL してもワークフローは落とさない。実行コマンド・終了
コード・出力（末尾 200 行）を `/tmp/probe-report.md` に書き出し、後続の
codex exec へ判定材料として渡す。Codex 側では「FAIL は破壊的変更の強い証拠だが、
CI 環境要因（認証・ネットワーク・npm 未公開バージョン）の可能性もあるため、チェンジログと
ソースを突き合わせて原因を特定してから対応を決める」よう指示している。新バージョン CLI の
npm インストール自体に失敗した場合は、その旨を report に記録してプローブは未実行のまま
ステップ成功扱いにする（codex タグ `rust-vX.Y.Z` は `${NEW_VERSION#rust-v}` で npm 版へ変換）。

**ローカルでの手動実行**:

```
cargo test upstream_probe_ -- --ignored
```

`#[ignore]` を付けているため通常の `cargo test` では走らない。実バイナリが PATH に無い
場合は分かりやすいメッセージで panic する。別パスのバイナリを使うときは
`ADDNESS_PROBE_CODEX_BIN` / `ADDNESS_PROBE_CLAUDE_BIN` で差し替える。

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

## Codex が出した PR のレビュー観点

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
