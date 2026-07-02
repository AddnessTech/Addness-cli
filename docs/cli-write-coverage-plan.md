# Addness CLI 書き込み機能カバレッジ計画

## ゴール
Addness フロントエンド (`vision-todo-frontend`) からユーザーが実行できる **書き込み系操作（POST/PUT/PATCH/DELETE）すべて** を、CLI (`addness ...`) からも実行可能にする。TUI（ratatui）は対象外、純 CLI サブコマンド拡張のみ。

## 現状サマリ

| | 数 |
|---|---|
| バックエンド書き込みエンドポイント | 約 200 |
| フロントから叩かれている書き込み API | 約 120 |
| CLI 実装済み書き込みサブコマンド | 58 |

### CLI 実装済み（書き込み）
- `goal create` / `goal update` / `goal delete` / `goal archive` / `goal unarchive` / `goal restore`
- `goal duplicate` / `goal move` / `goal share create` / `goal share revoke`
- `goal alias add` / `goal alias rm` / `goal alias reorder`
- `comment create` / `comment update` / `comment delete` / `comment resolve` / `comment unresolve` / `comment react` / `comment attachment rm`
- `deliverable add` / `deliverable update` / `deliverable rename` / `deliverable move` / `deliverable rm` / `deliverable batch-move` / `deliverable batch-rm`
- `assignment add` / `assignment update` / `assignment rm` / `assignment transfer`
- `kpi add` / `kpi update` / `kpi rm`
- `link pr` / `link progress`
- `org create` / `org update` / `org rm` / `org set-context`
- `member update` / `member pin` / `member unpin` / `member rm` / `member admin grant` / `member admin revoke` / `member set-source-org`
- `invitation create` / `invitation resend` / `invitation revoke` / `invitation accept` / `invitation link create` / `invitation link deactivate`
- `today add` / `today done` / `today reopen` / `today status`
- `notification send`
- `org switch`（書き込みではないが状態変更）

## 全体ギャップ表（リソース単位、フロント書き込み機能 vs CLI 実装状況）

凡例: ✅ = CLI 実装済 / 🔴 = 未実装

### Goal / Objective
| 操作 | エンドポイント | CLI |
|---|---|---|
| create | POST /v2/objectives | ✅ |
| update (主要フィールド) | PATCH /v2/objectives/:id | ✅ (title/status/DoD/body/dueDate) |
| delete | DELETE /v2/objectives/delete | ✅ |
| archive | POST /v2/objectives/archive | ✅ |
| unarchive | POST /v2/objectives/unarchive | ✅ |
| restore | POST /v2/objectives/restore | ✅ |
| changeParent | POST /v2/objectives/:id/parent | ✅ |
| duplicate | POST /v2/objectives/:id/duplicate | ✅ |
| text-patch (description 部分更新) | POST /v2/objectives/:id/text-patch | 🔴 |
| share create | POST /v1/objectives/:id/share | ✅ |
| share revoke | DELETE /v1/objectives/:id/share | ✅ |
| alias create | POST /v1/objectives/:id/aliases | ✅ |
| alias reorder | PATCH /v1/objectives/:id/aliases/reorder | ✅ |
| alias delete | DELETE /v1/objectives/:id/aliases/:aliasId | ✅ |
| bulk_replace (v1) | PUT /v1/objectives/:id/bulk_replace | 🔴 |

### Comment
| 操作 | エンドポイント | CLI |
|---|---|---|
| create | POST /v1/comments | ✅ |
| update | PUT /v1/comments/:id | ✅ |
| delete | DELETE /v1/comments/:id | ✅ |
| resolve | PATCH /v1/comments/:id/resolve | ✅ |
| unresolve | PATCH /v1/comments/:id/unresolve | ✅ |
| addReaction | POST /v1/comments/:id/reactions | ✅ |
| deleteAttachment | DELETE /v1/comments/:id/attachments/:attachmentId | ✅ |

### Deliverable (Outcome)
| 操作 | エンドポイント | CLI |
|---|---|---|
| create | POST /v1/objectives/:id/deliverables | ✅ (add) |
| update | PATCH /v1/objectives/:id/deliverables/:id | ✅ |
| rename | PATCH .../rename | ✅ |
| move | PATCH .../move | ✅ |
| delete | DELETE .../:id | ✅ |
| batch_move | POST .../batch_move | ✅ |
| batch_delete | POST .../batch_delete | ✅ |

### Assignment（メンバーアサイン・権限）
| 操作 | エンドポイント | CLI |
|---|---|---|
| create | POST /v2/objectives/:id/assignments | ✅ |
| update | PATCH .../:assignmentId | ✅ |
| delete | DELETE .../:assignmentId | ✅ |
| transferOwnership | PUT /v2/objectives/:id/transfer-ownership | ✅ |

### KPI（定量目標）
| 操作 | エンドポイント | CLI |
|---|---|---|
| create | POST /v2/objectives/:id/kpis | ✅ |
| update | PATCH /v2/objective-kpis/:id | ✅ |
| delete | DELETE /v2/objective-kpis/:id | ✅ |

### AI Thread（AIチャット）
- create / update / delete / chat / cancel
- edit-and-regenerate / trace revert
- share create / revoke
- question respond / tool-confirmation respond

### AI Background Tasks（11 種）
すべて POST `/v2/objectives/:id/ai-*`:
- ai-health-check / ai-goal-tree / ai-completed-review / ai-bulk-decompose
- ai-create-plan / ai-code-research / ai-comment-summary / ai-web-research
- ai-discussion-to-deliverable / ai-schedule-check / ai-recursive-assign

### AI Agent
- CRUD / uploadAvatar
- skill assign/unassign
- MCP connection: create/delete/test/sync/toggle-tool
- credential: save/delete (per service)
- delegate task

### Organization
- create / update / delete / uploadLogo / updateContext
- AI schedule settings (PUT)

### Member
- update / pin / delete / uploadAvatar
- assign/revoke admin
- setSourceOrganization
- streak share link create/revoke

### Invitation
- create / resend / revoke
- inviteLink create / deactivate
- accept (token / invId)

### Recurring Goal
- create / update / delete (`/v2/objectives/:id/recurring`)

### Slack / Sheet Binding
- Slack binding create / delete
- Sheet binding create / delete / createForServiceAccount

### Meeting Note
- transcribe / summarize / post-minutes / create-goals
- minute CRUD

### Personal（個人ページ）
- today/append / text-patch / days/append
- agent-sessions create/update
- projects create/update

### Notification
- mark-read / mark-unread / mark-all-read

### Integration / External Credential
- credential save/delete (per service)
- Slack installation delete
- GitHub installation delete / repo toggle
- Google disconnect
- LINE link / unlink

### MCP Connection (Organization level)
- create / update / delete
- test / sync / oauth start
- toggle tool

### Skill / Tool（組織レベル）
- skill: CRUD
- tool: CRUD

### API Key
- create / revoke

### Goal Execution（日次ゴール実行）
- generate / update
- create objective from execution

### AI Plan Subscription
- register / cancel / change-plan / change-mode / end-trial
- cancel-scheduled-downgrade

### その他
- Notification settings
- ObjectiveAISchedule upsert
- Push token register
- User settings update
- Referral link
- Share tree create/revoke
- Goal AI followup schedule

---

## 共通基盤（Phase 0、各 Phase の前提）

1. **API クライアント拡充**
   - 各エンドポイント用の Rust 関数を `src/api/` 配下に追加（既存パターンに揃える）
   - レスポンスは `serde::Deserialize` 型として定義
2. **入力フォーマット規約**
   - 短い値はフラグ（`--title`、`--body`）
   - 長文はファイル指定（`--body-file`、stdin パイプ）も許可
   - JSON 入力（`--json '{"...": "..."}'`）は複雑な構造のリソースのみ
3. **出力フォーマット規約**
   - 人間用テーブル出力（既存 `output.rs` 利用）
   - `--json` でマシン用 JSON 出力（AI 連携のために MEMORY.md でも明示済み）
4. **共通フラグ**
   - `--org <id>` でデフォルト組織を上書き
   - `--force` で削除系の確認スキップ（既存 `goal delete` と揃える）
5. **エラーハンドリング**
   - 既存の `anyhow` ベース運用に合わせる

---

## Phase 1: Goal + Comment（最優先・今回ターゲット）

**目的**: ユーザーが Goal 上で設計コンテキストを溜める運用 (MEMORY.md 記載) をフル CLI で完結できるようにする。

### 1.1 Goal 追加コマンド

実装ファイル: `src/cli/commands/goal.rs` （既存に追記）

| サブコマンド | 引数 | API |
|---|---|---|
| `goal archive <ID>` | `<ID>` | POST /v2/objectives/archive (body: `{ids:[id]}`) |
| `goal unarchive <ID>` | `<ID>` | POST /v2/objectives/unarchive |
| `goal restore <ID>` | `<ID>` | POST /v2/objectives/restore |
| `goal duplicate <ID>` | `<ID>` [`--with-children`] | POST /v2/objectives/:id/duplicate |
| `goal move <ID> --parent <PID>` | `<ID> --parent` | POST /v2/objectives/:id/parent |
| `goal patch <ID>` | `--description <md>` または `--description-file <path>` | POST /v2/objectives/:id/text-patch |
| `goal share create <ID>` | `<ID>` | POST /v1/objectives/:id/share |
| `goal share revoke <ID>` | `<ID>` | DELETE /v1/objectives/:id/share |
| `goal alias add <ID>` | `<ID> --name <n>` | POST /v1/objectives/:id/aliases |
| `goal alias rm <ID> <AID>` | `<ID> <ALIAS_ID>` | DELETE .../aliases/:aliasId |
| `goal alias reorder <ID>` | `<ID> --order id1,id2,...` | PATCH .../aliases/reorder |

**実装メモ**:
- `goal patch` の text-patch は description の差分更新が必要。シンプルな実装としては「現在の description を取得 → ユーザー指定の text-patch ペイロードを POST」。ペイロード型はバックエンドの `objectiveV2Handler.PatchText` を確認して合わせる。
- `goal update` は現状 title/status のみだが、`goal patch` を別サブコマンドにする方が混乱が少ない（既存挙動温存）。
- `goal duplicate` のレスポンス（新 ID）はテーブル/JSON 両方で表示。
- archive 系はバックエンドが `{ids:[...]}` 配列を受ける場合、CLI も `--ids` フラグで複数指定対応を検討。

### 1.2 Comment 追加コマンド

実装ファイル: `src/cli/commands/comment.rs` （既存に追記）

| サブコマンド | 引数 | API |
|---|---|---|
| `comment update <ID>` | `--body` または `--body-file` | PUT /v1/comments/:id |
| `comment delete <ID>` | [`--force`] | DELETE /v1/comments/:id |
| `comment resolve <ID>` | `<ID>` | PATCH /v1/comments/:id/resolve |
| `comment unresolve <ID>` | `<ID>` | PATCH /v1/comments/:id/unresolve |
| `comment react <ID> --emoji <e>` | `<ID> --emoji` | POST /v1/comments/:id/reactions |
| `comment attachment rm <CID> <AID>` | `<CID> <AID>` | DELETE .../:id/attachments/:attachmentId |

**実装メモ**:
- 添付ファイルの追加は `comment create` 側の拡張（`--attach <path>`）が望ましいが、現状フロントの送信方式（multipart? JSON+blob?）を要確認。
- `react` の emoji 形式（Unicode? short name?）はバックエンドの `commentHandler.AddReaction` のリクエスト型に合わせる。

### 1.3 Phase 1 規模見積もり

- 追加サブコマンド数: 17
- 追加コード量: 約 600〜800 行（API クライアント関数 + clap 定義 + 実装 + テスト最低限）
- 既存ファイルへの追記中心、新規ファイル不要見込み

---

## Phase 2: Deliverable + Assignment + KPI

新規追加サブコマンド見込み: 14

| 領域 | サブコマンド |
|---|---|
| Deliverable | `rename`, `move`, `rm`, `update`, `batch-move`, `batch-rm` |
| Assignment | `assignment add/update/rm <GoalID>`, `assignment transfer <GoalID> --to <user>` |
| KPI | `kpi add/update/rm` |

実装ファイル: `deliverable.rs` 拡張 + 新規 `assignment.rs`、`kpi.rs`

規模見積もり: 約 500〜700 行

---

## Phase 3: AI 関連

### 3.1 AI Thread
新規ファイル `src/cli/commands/ai.rs` を作る方針。
サブコマンド: `ai thread create/list/update/rm/chat/cancel`、`ai thread share`、`ai trace revert`

### 3.2 AI Background Tasks (11 種)
統一サブコマンド: `addness ai run <task-type> --goal <ID> [--params <json>]`
- task-type: health-check / goal-tree / completed-review / bulk-decompose / create-plan / code-research / comment-summary / web-research / discussion-to-deliverable / schedule-check / recursive-assign

### 3.3 AI Agent
新規ファイル `src/cli/commands/agent.rs`
サブコマンド: `agent create/update/rm/list`、`agent skill add/rm`、`agent mcp add/rm/test/sync/toggle`、`agent credential set/rm`、`agent delegate <agentID> --task <...>`

規模見積もり: 約 1000〜1500 行（最大の Phase）

---

## Phase 4: 組織・メンバー・招待

### 4.1 Organization
拡張: `org create/update/rm/upload-logo/set-context`

### 4.2 Member
新規 `src/cli/commands/member.rs`:
`member list/update/pin/rm/upload-avatar/admin grant|revoke/set-source-org`

### 4.3 Invitation
新規 `src/cli/commands/invitation.rs`:
`invitation create/resend/revoke/accept`、`invite-link create/deactivate`

規模見積もり: 約 700〜900 行

---

## Phase 5: 連携 (Integration / MCP / Skill / Tool / Binding)

- `slack-binding add/rm`
- `sheet-binding add/rm`（既存サービスアカウント対応含む。MEMORY.md `sheets_service_account_setup.md` 参照）
- `mcp connection add/update/rm/test/sync/oauth-start/toggle` (Organization レベル)
- `skill create/update/rm` (既存 `skills.rs` 拡張)
- `tool create/update/rm`
- `integration credential save/rm`（Slack/Google/GitHub/LINE）

規模見積もり: 約 800〜1000 行

---

## Phase 6: その他（Personal / Meeting / Notification / Subscription / API Key 等）

- `personal today/days/projects/agent-session` 系
- `meeting note transcribe/summarize/post-minutes/create-goals`、`minute CRUD`
- `notification mark-read/all-read`
- `subscription register/cancel/change-plan/end-trial`
- `apikey create/revoke`
- `recurring create/update/rm`（Goal の繰り返し設定）
- `goal-execution generate/update`

規模見積もり: 約 800〜1200 行

---

## 全体規模感

| Phase | 領域 | サブコマンド追加数 | 推定行数 | セッション数目安 |
|---|---|---|---|---|
| 0 | 共通基盤 | - | 200 | 0.5 |
| 1 | Goal + Comment | 17 | 600-800 | 1 |
| 2 | Deliverable/Assignment/KPI | 14 | 500-700 | 1 |
| 3 | AI 関連 | 30+ | 1000-1500 | 2 |
| 4 | 組織・メンバー・招待 | 20+ | 700-900 | 1-2 |
| 5 | 連携 | 25+ | 800-1000 | 1-2 |
| 6 | その他 | 25+ | 800-1200 | 1-2 |
| **合計** | | **130+** | **約 4600〜6300** | **7-10** |

---

## 設計上の決定事項・未決事項

### 決定済み
- TUI は追加しない（純 clap サブコマンド拡張）
- `--json` 出力を全コマンドで一級サポート（AI 連携 / スクリプト用途）
- 既存 `goal update` の挙動（title/status のみ）は破壊変更せず、より広い更新は `goal patch` で導入
- 削除系は `--force` で確認スキップ、デフォルトはプロンプト確認（`goal delete` の挙動踏襲）

### 要決定
1. **重い操作（AI Background Tasks）の応答待ち**: 同期/非同期どちらをデフォルトにするか
   - 案A: デフォルト非同期、`--wait` で待機
   - 案B: デフォルト同期、`--detach` で非同期
2. **stdin/EDITOR 入力**: 長文 description / comment body は `$EDITOR` を起動して編集できるようにするか
3. **ID 短縮**: フロント URL 上の UUID をそのまま使うか、`addness goal current` のような暗黙コンテキスト ID を導入するか
4. **アクセス制御の表示**: Assignment などフロントで権限管理 UI のあるものは、CLI でも組織メンバー検索 (`addness member search`) と組み合わせて自然に書けるようにするか
5. **`bulk_replace` (v1) / `text-patch` の AI 専用度**: フロント未使用ならスキップ判定
6. **添付ファイル（comment attachment / agent avatar 等）**: multipart アップロードの実装方針（既存 `deliverable add --file` パターンを流用可能か）

---

## 次のアクション

1. ✅ 本計画書をレビュー・調整
2. （任意）この内容を Addness 本体 Goal の description として保存（MEMORY.md の運用パターン）
3. Phase 0（共通基盤）の小さな PR を切る
4. Phase 1（Goal + Comment）の実装着手
5. 各 Phase 完了ごとに README とヘルプを更新

## 参照
- バックエンドエンドポイント一覧の根拠: `vision-todo-backend/` のルーター定義
- フロントエンド API クライアントの根拠: `vision-todo-frontend/src/lib/api/`
- MEMORY.md 関連エントリ: `project_cli.md`, `sheets_service_account_setup.md`
