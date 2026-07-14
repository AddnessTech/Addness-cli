# 上流統合サーフェス定義（upstream-surface）

本リポジトリの TUI は **Claude Code CLI**（`anthropics/claude-code`）と
**Codex CLI**（`openai/codex`）の 2 つをサブプロセスとして起動・統合している。
このドキュメントは、上流リリースの変更が本リポジトリに「関係するか」を
自動化エージェント（`.github/workflows/upstream-sync.yml`）が判定するための正本である。

> **ソースの場所について**: バックエンド統合の正本は `src/tui/agent/`
> （`mod.rs` / `claude.rs` / `codex.rs`）にある。`feat/claude-code-backend`
> ブランチのマージ前の main では、簡略版が `src/tui/codex_pane.rs` にあり
> Claude Code バックエンドは未導入。ファイルが見つからない場合は
> `git grep` で下記の関数名・enum 名を探すこと。

---

## 1. Claude Code CLI との統合サーフェス（`src/tui/agent/claude.rs`）

### 1.1 使用中の CLI フラグ（`claude::exec_args`）

ターン実行は `claude -p` の非対話モードで行い、プロンプトは stdin へ渡す。
以下のフラグの追加・変更・廃止・挙動変更は**すべて関係あり**。

| フラグ | 用途 | 組み立て箇所 |
|---|---|---|
| `-p` | 非対話（print）モード | `exec_args` |
| `--output-format stream-json` | イベントを JSON Lines で受信 | `exec_args` |
| `--verbose` | stream-json の全イベント出力に必要 | `exec_args` |
| `--resume <session_id>` | 2 ターン目以降のセッション継続 | `exec_args` |
| `--fork-session` | resume 時のセッション複製 | `exec_args` |
| `--model <name>` | モデル指定（下記 1.2） | `exec_args` |
| `--effort <level>` | effort 指定（下記 1.3） | `exec_args` |
| `--permission-mode <mode>` | 権限モード指定（下記 1.4） | `exec_args` |
| `--dangerously-skip-permissions` | 原則許可（sticky / one-shot 昇格。壊滅的な削除等は確認され得る） | `exec_args` |
| `--add-dir <dir>` | 書込許可ディレクトリ追加（複数可） | `exec_args` |
| `--append-system-prompt <text>` | Addness 手順を毎ターン注入 | `exec_args` |

環境変数: `CLAUDE_CONFIG_DIR`（`claude::config_dir`）。

### 1.2 モデル選択肢（F2 / `/model`）— `ClaudeModelChoice`

`config`（フラグなし）/ `fable` / `opus` / `sonnet` / `haiku`。
上流で**新モデルエイリアスの追加・廃止**があれば enum と `parse_model_choice` の更新が必要。
任意文字列は `model_override` としてそのまま `--model` に渡るため、フルモデル ID の
追加だけなら必須対応ではない（エイリアス追加は対応する）。

### 1.3 effort 選択肢（F3 / `/effort` 相当）— `ClaudeEffortChoice`

`config` / `low` / `medium` / `high` / `xhigh` / `max`。
`--effort` の選択肢追加・廃止・名称変更は関係あり（`parse_effort_choice` も更新）。

### 1.4 permission-mode 選択肢（F4 / `/permissions`）— `ClaudePermissionMode`

`config` / `plan` / `acceptEdits` / `dontAsk` / `bypassPermissions`。
`--permission-mode` の選択肢追加（例: 上流 2.1.200 の `manual` 追加のようなケース）や
名称変更は関係あり（`parse_permission_mode` も更新）。
関連: `PermissionEscalation` / `escalation_for_denials`（Edit/Write/MultiEdit/NotebookEdit
のみの拒否なら `acceptEdits`、それ以外を含めば `--dangerously-skip-permissions` へ昇格）。

### 1.5 パースしている stream-json イベント

防御的パーサ（`serde_json::Value` ベース）だが、**スキーマ変更は関係あり**:

- `system` / subtype `init` — `session_id`（snake_case。永続 jsonl は `sessionId`）
  → `event_type` / `event_subtype` / `session_id`
- `assistant` — `message.content[]` の `text` / `thinking` / `tool_use`（`name`, `input`）
  → `assistant_blocks` / `tool_use_summary`
- `user` — `message.content[]` の `tool_result`（`content`, `is_error`）→ `tool_results`
- `result` — `result` / `is_error` / `subtype` / `usage`（`input_tokens`,
  `output_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`）/
  `total_cost_usd` / `permission_denials[]`（`tool_name`, `tool_input`）
  → `parse_result` / `parse_denial` / `usage_summary`

`tool_use_summary` が特別扱いするビルトインツール名（改名・追加は関係あり）:
`Bash`, `BashOutput`, `Read`, `Write`, `Edit`, `MultiEdit`, `NotebookEdit`,
`Glob`, `Grep`, `WebFetch`, `WebSearch`, `Task`。

### 1.6 セッションファイル探索

- ルート: `CLAUDE_CONFIG_DIR` または `~/.claude`（`config_dir`）
- パス: `~/.claude/projects/<cwd スラッグ>/*.jsonl`
  （スラッグは `/`, `.`, `_` → `-` 置換。`cwd_slug`）
- 各 jsonl から `type == "user"` 行の `message.content` と `timestamp` を読む
  （`load_session_candidates_from` / `session_candidate_from_file` / `first_user_text`）

**セッションファイルの置き場所・スラッグ規則・レコード形式の変更は関係あり。**

### 1.7 常駐モード（stream-json 双方向）— `claude::resident_args`

多ターンを 1 プロセスで回す常駐モードでは、`exec_args`（1.1）に加えて双方向
stream-json 入力とその場承認用の追加フラグを渡す（`src/tui/agent/claude.rs` の
`resident_args`。プロセス起動は `mod.rs::spawn_claude_resident`）。

| フラグ | 用途 |
|---|---|
| `--input-format stream-json` | ターン入力を JSON Lines で送る（常駐固有） |
| `--permission-prompt-tool stdio` | `can_use_tool` 承認を stdio 経由で受ける隠しフラグ |

`--output-format stream-json` / `--include-partial-messages` / `--verbose` /
`--resume` / `--fork-session` / `--model` / `--effort` / `--permission-mode` /
`--allowedTools` / `--add-dir` / `--append-system-prompt` は 1.1 と共通のヘルパで組む。
**これらのフラグの改名・廃止・挙動変更は関係あり**（常駐 spawn が起動時に失敗し、
ワンショットへ恒久フォールバックする）。

---

## 2. Codex CLI との統合サーフェス（`src/tui/agent/mod.rs`）

### 2.1 CLI 引数ビルダー（15 関数）

mod.rs 1009〜1158 行付近と 9381〜9649 行付近。これらが組むサブコマンド・フラグの
追加・変更・廃止は関係あり:

- `codex_named_subcommand_args` / `codex_command_with_args` /
  `codex_named_subcommand_args_with_settings` — `/codex` 等の任意サブコマンド委譲
- `codex_exec_args` / `codex_exec_resume_args` — `exec --json` / `exec resume --json`
  によるターン実行（プロンプトは `-` で stdin）
- `codex_root_interactive_args` / `codex_root_resume_args` /
  `codex_root_session_command_args` — 対話モード起動・resume
- `codex_fork_args` / `codex_session_admin_args` — fork・セッション管理
- `codex_review_args` / `codex_exec_review_args` — `review` / `exec review --json`
- `codex_apply_args` — `apply <task_id>`
- 共通: `push_global_exec_settings` / `push_optional_exec_settings`

使用フラグ一覧: `--json`, `-a/--ask-for-approval`, `-s/--sandbox`,
`--dangerously-bypass-approvals-and-sandbox`, `--dangerously-bypass-hook-trust`,
`--strict-config`, `--search`, `--oss`, `--local-provider`, `-p <profile>`,
`--add-dir`, `-c key=value`（`developer_instructions`, `model_reasoning_effort`,
`memories.use_memories`, `memories.generate_memories`）, `--enable` / `--disable`,
`--ignore-user-config`, `--ignore-rules`, `--skip-git-repo-check`, `--ephemeral`,
`-m/--model`, `-i/--image`, `--output-schema`, `-o/--output-last-message`,
`--color`, `-C <cwd>`, `--remote`, `--remote-auth-token-env`, `--no-alt-screen`。

### 2.2 選択肢 enum（mod.rs 346〜634 行付近）

| enum | 選択肢 | 対応フラグ |
|---|---|---|
| `CodexModelChoice` | config / gpt-5.5 / gpt-5 / o3 | `-m` |
| `CodexReasoningChoice` | config / low / medium / high / xhigh | `-c model_reasoning_effort=` |
| `CodexApprovalChoice` | config / untrusted / on-request / on-failure / never | `-a` |
| `CodexSandboxChoice` | read-only / workspace-write / danger-full-access | `-s` |
| `CodexLocalProviderChoice` | config / lmstudio / ollama | `--local-provider` |
| `CodexColorChoice` | never / auto / always | `--color` |

上流での選択肢の追加・廃止・名称変更は関係あり（各 `parse_*_choice` も更新）。
設定の集約は `CodexExecSettings`。

### 2.3 パースしている exec --json イベント

`thread.started`（`thread_id` / `threadId` / `id`）、`turn.started` /
`turn_completed`、`agent_message` / `agent_message.delta`、`item.completed`、
`token_count` / `event_msg`（payload 内 `token_count`, `total_token_usage`,
`last_token_usage`, `model_context_window`）等。イベント名・フィールドの
スキーマ変更は関係あり（`handle_stdout_line` 経由のハンドラ群）。

### 2.4 セッション・skill 探索

- ルート: `CODEX_HOME` または `~/.codex`（`codex_home_dir`、mod.rs 1254 行付近）
- セッション: `~/.codex/sessions/` 配下のメタファイル + セッション index
  （`load_codex_session_candidates_from` / `read_codex_session_index` /
  `read_codex_session_meta_files`、1301〜1414 行付近）
- skill: `<cwd>/.codex/skills`, `<cwd>/.agents/skills`, `~/.codex/skills`
  （`codex_skill_roots`）

### 2.5 スラッシュコマンド委譲

TUI のスラッシュコマンド正本は `SLASH_COMMANDS`（mod.rs 32 行付近）。うち
`/review` `/apply` `/cloud` `/codex` `/profile` `/config` 等は codex CLI へ 1:1 委譲
（`CODEX_ONLY_SLASH_COMMANDS`, 99 行付近）。**上流 CLI のサブコマンド追加**は、
委譲コマンド追加候補として関係あり。

### 2.6 app-server（常駐モード）の JSON-RPC サーフェス（`src/tui/agent/codex_appserver.rs`）

ワンショット（`codex exec --json`）とは別に、`codex app-server`（改行区切り JSON /
JSON-RPC 2.0 / stdio）を常駐プロセスとして起動し 1 プロセスで多ターンを回す
（プロセス起動は `mod.rs::spawn_codex_appserver`、生成/パースは `codex_appserver.rs`）。

**クライアント発のメソッド（`*_request` / `*_notification`）**:

- `initialize`（`experimentalApi` 有効化・`optOutNotificationMethods` で雑多な通知を抑制）
- `initialized`（通知）
- `thread/start` / `thread/resume`（`cwd` / `model` / `approvalPolicy` / `sandbox` /
  `developerInstructions`）
- `turn/start`（`input`: text / localImage、`effort`）/ `turn/interrupt`
- `thread/settings/update`（`model` / `effort` / `approvalPolicy` / `sandboxPolicy`）

**サーバ発リクエスト（承認）**: `item/commandExecution/requestApproval` /
`item/fileChange/requestApproval` / `item/permissions/requestApproval`
（`availableDecisions` / `permissions` をエコーバックして応答）。

**消費している通知**: `turn/started` / `turn/completed`（`turn.status`）/
`item/agentMessage/delta` / `item/commandExecution/outputDelta` /
`item/reasoning/summaryTextDelta` / `item/reasoning/textDelta` / `item/started` /
`item/completed`（`item.type`: commandExecution / agentMessage / reasoning /
fileChange / mcpToolCall）/ `thread/tokenUsage/updated` / `error`。

**実測知見**: メッセージの `jsonrpc` フィールドは codex **0.142.5 以降省略されうる**
（`looks_like_jsonrpc` / `parse_message` は欠落を許容し、明示されている場合のみ
`"2.0"` 以外を弾く実装になっている）。この種の「チェンジログに現れない挙動変更」は
`.github/workflows/upstream-sync.yml` のプローブ（`upstream_probe_codex_appserver_handshake`）
が新バージョン実バイナリで実測検知する。

**メソッド名・params 形状・通知種別・`jsonrpc` 省略などプロトコルの変更は関係あり**
（ハンドシェイクが壊れると常駐がワンショットへフォールバックする）。

---

## 3. 関連性の判定ガイドライン

### 関係あり（PR または Issue の対象）

- 上記 1.1 / 2.1 に列挙した **CLI フラグ・サブコマンドの追加・変更・廃止・非推奨化**
- **モデル / effort / reasoning / permission-mode / approval / sandbox の選択肢**の追加・廃止・改名
- **stream-json / exec --json のイベントスキーマ変更**（イベント種別・フィールドの追加改名、
  usage・cost・permission_denials 形式の変更）
- **セッションファイル**の形式・置き場所・スラッグ規則・index 形式の変更
- **設定ディレクトリ環境変数**（`CLAUDE_CONFIG_DIR` / `CODEX_HOME`）まわりの変更
- 上流 CLI の**スラッシュコマンド / サブコマンド追加**（TUI パレットへの追加候補）
- exit code・stdin/stdout プロトコルなど**サブプロセス制御に影響する挙動変更**

### 関係なし（実装不要。ログに理由を残して終了）

- IDE 拡張（VS Code / JetBrains）、Web 版、デスクトップアプリ、モバイル
- SDK（Agent SDK / TypeScript / Python）、GitHub Actions、Docker イメージのみの変更
- 対話 TUI の見た目・スクリーンリーダー・テーマ等、**非対話モードに影響しない UX 修正**
- インストーラ・自動アップデート・テレメトリ・課金・レート制御の内部改善
- 上流内部のバグ修正で、本リポジトリが依存するフラグ・スキーマの意味が変わらないもの
- ドキュメント・CI・社内向けの修正

### 判断に迷う場合

「本リポジトリがそのフラグ/イベント/パスを実際に使っているか」を `git grep` で確認する。
使っていなければ関係なし。使っており挙動が変わるなら関係あり。確信が持てない・影響が
大きい場合は PR ではなく Issue（label: `upstream-sync`）で人間に委ねる。
