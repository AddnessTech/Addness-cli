# CLI エンドポイントカバレッジ・ギャップ対応表

親ゴールDoD「vision-todo-backend最新mainのユーザー向けエンドポイントすべてをCLIから叩ける」の実装計画の土台。
[`docs/backend-endpoint-inventory.md`](./backend-endpoint-inventory.md)（バックエンド659ルート棚卸し）と、CLI実装断面（`src/cli/commands/*.rs` 80サブコマンド、`src/api/client/*.rs` 実HTTPエンドポイント約53種）を突き合わせ、グループごとに「バックエンドendpoint ⇔ 既存CLIコマンド ⇔ 状態」を対応付ける。

対応付けの根拠は、`/tmp/vtb-main`（vision-todo-backend読み取り専用worktree、Go/Gin、`presentation/routes/api.go`）のハンドラ実装と、本リポジトリ`src/api/client/*.rs` / `src/cli/commands/*.rs` の実装内容を実際に突き合わせて判定した。

---

## 0. 「対象外」の定義

CLIから叩く実装対象として扱わない（DoDの分母から除外する）エンドポイント群を、以下6カテゴリとして明文化する。各カテゴリの該当エンドポイントは末尾の「対象外リスト」に一覧化する。

| # | カテゴリ | 定義 | 判定理由 |
|---|---|---|---|
| 1 | Webhook / 外部コールバック受信専用 | 決済・認証・Git・チャット等の外部サービスから一方的にPOSTされるwebhook、およびOAuthフローのブラウザリダイレクト着地点（`*/oauth/callback`, `*/callback`） | 署名/state/HMAC等で検証される受信専用エンドポイントであり、CLIが能動的に呼び出す運用が存在しない |
| 2 | 内部/デバッグ用 | MCPプロトコル本体（`/mcp*`、`.well-known/*`、`/authorize`、`/register`、`/token`）、内部プロキシ・内部監査（chat admin export、mcp-uploads）、サービス間認証（`X-Addness-App-Key`）専用エンドポイント、WebSocketプロトコル（`/chat/ws`）、リリース前削除予定の内部検証送信（`report-schedule/test`） | 人間ユーザーが直接叩くCLIコマンドの対象ではなく、別プロトコル・別サービスが利用する内部経路 |
| 3 | 検証用並走ルート | `/api/v2/organizations/:id/validate/...`、`/api/v2/personal/validate/...`、`/api/v2/codex/validate/...`、`/api/v2/ai-todo-chat/validate/...`、`/api/v2/validate/objective/create` | `internal/goalexecutionvalidate` / `internal/aitodochatvalidate` という別モジュールによる、本番機能と同一機能セットのA/B検証専用の並走実装。本番側（validateなしパス）を実装すれば機能的に等価 |
| 4 | ALBエイリアス | `/api/ai/v1/*`, `/api/ai/v2/*` | ALBパスマッチングによる段階移行用の別名ルート。ハンドラは`/api/v1/team/*`・`/api/v2/*`と共用（実体は同一機能） |
| 5 | admin専用 | `Clerk+Admin`認証（社内管理者専用）のエンドポイント（管理者ダッシュボード、サブスク強制更新、グローバル使用量集計、紹介ログ閲覧） | 一般ユーザー向けCLIの対象外。社内オペレーション専用ツール |
| 6 | v1レガシー重複（v2で代替実装済み） | v1エンドポイントのうち、同一機能のv2エンドポイントを既にCLIが実装済みのもの | 追加でv1側を実装する意味がない（新規実装が必要な場合は最初からv2を選ぶべき） |

上記6カテゴリのいずれにも該当しない未実装エンドポイントは、すべて「未実装（真のギャップ）」として扱う。

---

## サマリ：グループ別 状態内訳

| # | グループ | 総数（登録ベース） | 実装済み | 未実装 | 対象外 |
|---|---|---:|---:|---:|---:|
| 1 | システム / ヘルスチェック | 2 | 0 | 0 | 2 |
| 2 | 認証・MCP OAuth・APIキー・デスクトップ認証 | 20 | 3 | 8 | 9 |
| 3 | Webhook | 9 | 0 | 0 | 9 |
| 4 | 外部連携 | 47 | 0 | 40 | 7 |
| 5 | MCPプロトコル | 3 | 0 | 0 | 3 |
| 6 | ユーザー / ユーザー設定 | 9 | 0 | 9 | 0 |
| 7 | 組織 (Organization) | 33 | 8 | 20 | 5 |
| 8 | メンバー / メンバータグ / 招待 | 39 | 8 | 31 | 0 |
| 9 | ゴール/目標（v1+v2, KPI, Assignment, Sheets等） | 113 | 24 | 82 | 7 |
| 10 | 成果物 (Deliverable) | 11 | 9 | 2 | 0 |
| 11 | コメント / Goal Issue / 組織チャット | 56 | 10 | 43 | 3 |
| 12 | AIスレッド・エージェント | 55 | 0 | 53 | 2 |
| 13 | AIエージェントチャット | 16 | 0 | 13 | 3 |
| 14 | 通知 / 通知設定 / メール宛先 | 11 | 0 | 11 | 0 |
| 15 | 実行タブ・カレンダー | 51 | 1 | 26 | 24 |
| 16 | アクティビティログ | 4 | 0 | 4 | 0 |
| 17 | ミーティング | 30 | 0 | 30 | 0 |
| 18 | ストリーク | 8 | 0 | 8 | 0 |
| 19 | スキル / ツール | 20 | 0 | 20 | 0 |
| 20 | 個人スペース (Personal) | 21 | 0 | 21 | 0 |
| 21 | 検索 / 診断 / 紹介 / 請求書 / 共有 / インラインメディア | 24 | 0 | 23 | 1 |
| 22 | Codexジョブ | 16 | 0 | 16 | 0 |
| 23 | 管理者 (Admin) | 5 | 0 | 0 | 5 |
| 24 | ALB用エイリアスルート | 7 | 0 | 0 | 7 |
| | **合計** | **~610**※ | **63** | **~470** | **~77** |

※ グループ9の「AIバックグラウンドタスク23機能×v1/v2」を1機能=2登録として計上、実行タブのvalidate系（24本）を含む。棚卸し表側の659本という登録行ベースの総数とは、MCP Any(3本)・重複カウント方法の違いにより一致しない。

**全体として、ユーザー向け実質エンドポイントのうちCLIが実装済みなのは1割程度。** 特にAIスレッド/エージェント・AIチャット・通知・実行タブ・ミーティング・スキル/ツール・個人スペース・ストリーク・Codexジョブは実装ゼロで、goal-issue（現行のv2チャット型コメント基盤）も未着手。DoD達成には広範な新規実装が必要。

---

## 1. システム / ヘルスチェック

| Method | Path | CLIコマンド | 状態 |
|---|---|---|---|
| GET | /api/v1/health | - | 対象外（内部/デバッグ用: インフラヘルスチェック） |
| GET | /api/v1/health/ready | - | 対象外（内部/デバッグ用） |

## 2. 認証・MCP OAuth・APIキー・デスクトップ認証

| Method | Path | CLIコマンド | 状態 |
|---|---|---|---|
| GET | /.well-known/oauth-protected-resource(+`/*path`) | - | 対象外（内部/デバッグ用: MCPプロトコル） |
| GET | /.well-known/oauth-authorization-server(+`/*path`) | - | 対象外（内部/デバッグ用） |
| GET | /authorize | - | 対象外（内部/デバッグ用） |
| POST | /register | - | 対象外（内部/デバッグ用） |
| POST | /token | - | 対象外（内部/デバッグ用） |
| POST | /api/v1/team/mcp-oauth/code | - | 対象外（内部/デバッグ用） |
| POST | /api/v1/mcp/uploads/:ticket | - | 対象外（内部/デバッグ用） |
| POST | /api/v1/team/api-keys | - | 未実装 |
| GET | /api/v1/team/api-keys | - | 未実装 |
| DELETE | /api/v1/team/api-keys/:id | - | 未実装 |
| GET | /api/v1/auth/google/callback | - | 対象外（Webhook/外部コールバック） |
| POST | /api/v1/public/desktop/auth/installations/register | `addness login`（`login.rs`直接reqwest） | 実装済み |
| POST | /api/v1/public/desktop/auth/start-sessions | `addness login` | 実装済み |
| POST | /api/v1/public/desktop/auth/start-sessions/redeem | - | 未実装 |
| POST | /api/v1/public/desktop/auth/token-exchange | `addness login` | 実装済み |
| POST | /api/v1/team/desktop/auth/intents/:id/complete | - | 未実装 |
| GET | /api/v2/me/consents/:consentType | - | 未実装 |
| POST | /api/v2/me/consents | - | 未実装 |

## 3. Webhook

全9エンドポイント（univapay/polar/clerk/github/zoom/line/livekit/recall/google-drive）: **対象外（Webhook）**。定義上、外部サービスからの一方的な受信専用でありCLIの対象外。

## 4. 外部連携（Slack / Discord / GitHub / Google / LINE / Zoom / Codex Integrations）

| Method | Path | CLIコマンド | 状態 |
|---|---|---|---|
| GET | /api/v1/slack/oauth/callback | - | 対象外（Webhook/外部コールバック） |
| POST | /api/v1/slack/commands/notify | - | 対象外（Webhook: Slack署名検証の受信専用） |
| GET | /api/v1/discord/oauth/callback | - | 対象外（Webhook/外部コールバック） |
| POST | /api/v1/discord/link-channel | - | 対象外（Webhook: リンクコード認証の受信専用） |
| GET | /api/v1/team/zoom/oauth/callback | - | 対象外（Webhook/外部コールバック） |
| GET | /api/v1/github/callback | - | 対象外（Webhook/外部コールバック） |
| GET | /api/v2/codex/integrations/{oauth,slack,github}/callback（3本） | - | 対象外（Webhook/外部コールバック） |
| GET/DELETE | /api/v1/team/integrations/slack/connect, installations(+:id), destinations, channels(+:id/history) | - | 未実装（6本） |
| POST | /api/v1/team/integrations/slack/messages | - | 未実装 |
| GET/DELETE | /api/v1/team/integrations/discord/connect, installations(+:id, +:id/channels), destinations(PUT/GET/DELETE) | - | 未実装（7本） |
| GET/DELETE/PATCH | /api/v1/team/integrations/github/install, installation(GET/DELETE), repos(GET), repos/:id(PATCH) | - | 未実装（5本） |
| GET/POST/DELETE | /api/v1/team/integrations/line/friend-url, link-info, link(POST/DELETE), status | - | 未実装（5本） |
| POST/GET/DELETE | /api/v1/team/integrations/credentials（全4本） | - | 未実装 |
| GET/DELETE | /api/v1/team/integrations/google/connect, disconnect, picker-token | - | 未実装（3本） |
| GET/DELETE/POST | /api/v1/team/zoom/status, disconnect, auth/start, jobs一式(list/create/get/delete/stream/summary) | - | 未実装（8本） |
| POST | /api/v1/team/link-codes/slack, /discord | - | 未実装（2本） |
| GET/PUT/DELETE/POST | /api/v2/codex/integrations（list/connect/disconnect/oauth-start）, slack/github connect | - | 未実装（6本） |

**小計: 40本未実装、7本対象外（webhook/callback）。** 外部連携は現状CLIコマンドが1つも存在しない領域。実装にはOAuthブラウザ導線が絡むものが多く難度は高い。

## 5. MCPプロトコル

全3エンドポイント（`/mcp`, `/mcp/codex`, `/mcp/personal`）: **対象外（内部/デバッグ用）**。MCP Streamable HTTPの内部プロトコル本体であり、RESTのCLIコマンドとして実装する対象ではない（別途MCPクライアントが利用）。

## 6. ユーザー / ユーザー設定

| Method | Path | CLIコマンド | 状態 |
|---|---|---|---|
| GET | /api/v1/team/users/current | - | 未実装 |
| PUT | /api/v1/team/users/:id | - | 未実装 |
| GET | /api/v1/team/user_settings | - | 未実装 |
| PATCH | /api/v1/team/user_settings | - | 未実装 |
| GET | /api/v1/team/organization_members | - | 未実装 |
| GET | /api/v1/team/users | - | 未実装 |
| GET | /api/v1/team/users/:id | - | 未実装 |
| POST | /api/v1/team/users | - | 未実装 |
| DELETE | /api/v1/team/users/:id | - | 未実装 |

`addness user` 系コマンドがCLIに一切存在しない。自分のユーザー情報取得・更新（自己プロフィール系）は利用頻度が高く優先度が高い。

## 7. 組織 (Organization)

| Method | Path | CLIコマンド | 状態 |
|---|---|---|---|
| GET | /api/v1/team/organizations | - | 対象外（v1レガシー重複: v2 `/organizations/me` で `org list` が代替済み） |
| POST | /api/v1/team/organizations | `org create` | 実装済み |
| GET | /api/v1/team/organizations/:id | - | 未実装（`org get`相当なし） |
| DELETE | /api/v1/team/organizations/:id | `org rm` | 実装済み |
| GET | /api/v1/team/organizations/:id/organization_members | - | 対象外（v1レガシー重複: v2 members系で代替可能） |
| GET | /api/v1/team/organizations/:id/root_owner | - | 未実装 |
| GET | /api/v1/team/organizations/:id/accessible_root | - | 未実装 |
| GET | /api/v1/team/organizations/:id/ai_agent_member | - | 未実装 |
| GET | /api/v1/team/organizations/:id/access-state | - | 未実装 |
| POST | /api/v1/team/organizations/:id/push_tokens | - | 未実装 |
| POST | /api/v1/team/organization_subscriptions/register | - | 未実装 |
| PATCH | /api/v1/team/organization_subscriptions/:id/cancel | - | 未実装 |
| GET | /api/v1/team/organization_subscriptions/current | - | 未実装 |
| GET | /api/v2/organizations/me | `org list` | 実装済み |
| GET | /api/v2/organizations | - | 未実装（+Sub版一覧、`org list`は`/me`を使用） |
| GET | /api/v2/organizations/:id/objectives/tree | `goal list` / `summary` | 実装済み |
| PUT | /api/v2/organizations/:id/logo | - | 未実装 |
| PUT/PATCH | /api/v2/organizations/:id | `org update` | 実装済み |
| PUT/PATCH | /api/v2/organizations/:id/default-timezone | - | 未実装 |
| GET | /api/v2/organizations/:id/context | - | 未実装（`org get-context`相当なし） |
| PATCH | /api/v2/organizations/:id/context | `org set-context` | 実装済み |
| GET | /api/v2/organizations/:id/context/revisions | - | 未実装 |
| GET | /api/v2/organizations/:id/onboarding-billing-state | - | 未実装 |
| POST | /api/v2/organizations/:id/onboarding-billing/require | - | 未実装 |
| POST | /api/v2/organizations/:id/onboarding-billing/free | - | 未実装 |
| GET | /api/v2/organizations/:id/ai-schedule-settings | - | 未実装 |
| PUT | /api/v2/organizations/:id/ai-schedule-settings | - | 未実装 |
| GET | /api/v2/organizations/:id/ad-settings(+`/me`) | - | 未実装（2本） |
| PUT | /api/v2/organizations/:id/ad-settings(+`/me`) | - | 未実装（2本） |
| GET | /api/v2/organizations/:id/admin/check | - | 未実装 |
| GET | /api/v2/organizations/:id/current-member | - | 未実装 |

## 8. メンバー (Member) / メンバータグ / 招待 (Invitation)

| Method | Path | CLIコマンド | 状態 |
|---|---|---|---|
| GET | /api/v2/organizations/:id/members | `member list` | 実装済み |
| GET | /api/v2/organizations/:id/members/search | - | 未実装 |
| GET | /api/v2/organizations/:id/members/children | - | 未実装 |
| GET | /api/v2/organizations/:id/admins | - | 未実装 |
| GET | /api/v2/members/:id/delete-preview | - | 未実装 |
| DELETE | /api/v2/members/:id | `member rm` | 実装済み |
| PUT | /api/v2/members/:id/admin | `member admin grant` | 実装済み |
| DELETE | /api/v2/members/:id/admin | `member admin revoke` | 実装済み |
| GET | /api/v2/members | - | 未実装 |
| GET | /api/v2/members/children | - | 未実装 |
| GET | /api/v2/members/:id/objectives | - | 未実装 |
| PUT | /api/v2/members/:id/pin | `member pin` / `member unpin` | 実装済み |
| PUT | /api/v2/members/:id/avatar | - | 未実装 |
| PATCH | /api/v2/members/:id/source-organization | `member set-source-org` | 実装済み |
| PUT | /api/v2/members/:id | `member update` | 実装済み |
| GET | /api/v2/members/:id | - | 未実装（`member get`相当なし） |
| GET/POST/DELETE | /api/v2/.../member-tags（一覧/作成/削除）+ members/:id/tags（付与/一覧/解除） | - | 未実装（6本） |
| POST | /api/v1/team/organization_invitations | - | 対象外（v1レガシー重複: v2 `invitation create` で代替済み） |
| POST | /api/v1/team/organization_invitations/accept | - | 未実装（v2 `invitation accept`と機能重複だが用途別: v1トークン形式が異なる） |
| POST | /api/v1/team/organization_invitations/check_plan_upgrade | - | 未実装 |
| GET | /api/v2/invitations/:token | - | 未実装 |
| POST | /api/v2/invitations/accept | `invitation accept` | 実装済み |
| POST | /api/v2/invitations/:token/accept | - | 未実装 |
| GET | /api/v2/invitations/pending | - | 未実装 |
| POST | /api/v2/invitations/pending/:invId/access | - | 未実装 |
| POST | /api/v2/invitations/decline | - | 未実装 |
| POST | /api/v2/invite-links/:code/join | - | 未実装 |
| GET | /api/v2/organizations/:id/invited-members | - | 未実装 |
| GET | /api/v2/organizations/:id/invitation-overview | - | 未実装 |
| POST | /api/v2/organizations/:id/invitations | `invitation create` | 実装済み |
| POST | /api/v2/organizations/:id/invitations/:invId/resend | `invitation resend` | 実装済み |
| DELETE | /api/v2/organizations/:id/invitations/:invId | `invitation revoke` | 実装済み |
| POST | /api/v2/organizations/:id/invite-links | `invitation link create` | 実装済み |
| GET | /api/v2/organizations/:id/invite-links | - | 未実装（`invitation link list`相当なし） |
| DELETE | /api/v2/organizations/:id/invite-links/:linkId | `invitation link deactivate` | 実装済み |

## 9. ゴール/目標 (Objective/Goal) — v1 + v2 / AIバックグラウンドタスク / Assignment / Sheets紐付け

### 9-1. 基本CRUD・階層・共有・エイリアス

| Method | Path | CLIコマンド | 状態 |
|---|---|---|---|
| GET | /api/v1/team/objectives/search | `goal search` | 実装済み |
| GET | /api/v1/team/objectives/:id | - | 対象外（v1レガシー重複: v2 `goal get`で代替済み） |
| GET | /api/v1/team/objectives/:id/children | - | 対象外（v1レガシー重複: v2 `goal children`で代替済み） |
| POST | /api/v1/team/objectives | - | 対象外（v1レガシー重複: v2 `goal create`で代替済み） |
| PATCH | /api/v1/team/objectives/:id | - | 対象外（v1レガシー重複: v2 `goal update`で代替済み） |
| PATCH | /api/v1/team/objectives/:id/change_parent | - | 対象外（v1レガシー重複: v2 `goal move`で代替済み） |
| GET | /api/v1/team/objectives/:id/ancestors | - | 未実装（v2 ancestorsも未実装のためレガシー重複ではなく真の未実装） |
| GET | /api/v1/team/objectives/:id/flat_descendants | - | 未実装（同上） |
| POST | /api/v1/team/objectives/:id/share | `goal share create` | 実装済み |
| DELETE | /api/v1/team/objectives/:id/share | `goal share revoke` | 実装済み |
| GET | /api/v1/team/objectives/:id/aliases | - | 未実装（`goal alias list`相当なし） |
| POST | /api/v1/team/objectives/:id/aliases | `goal alias add` | 実装済み |
| PATCH | /api/v1/team/objectives/:id/aliases/reorder | `goal alias reorder` | 実装済み |
| DELETE | /api/v1/team/objectives/:id/aliases/:aliasId | `goal alias rm` | 実装済み |
| GET | /api/v1/team/objectives/:id/recurring-goals | - | 未実装 |
| POST | /api/v2/objectives | `goal create` | 実装済み |
| GET | /api/v2/objectives/:id | `goal get` | 実装済み |
| PATCH | /api/v2/objectives/:id | `goal update` | 実装済み |
| POST | /api/v2/objectives/:id/text-patch | - | 未実装（既知のギャップ、`cli-write-coverage-plan.md`で明示済み） |
| DELETE | /api/v2/objectives/delete | `goal delete` | 実装済み |
| DELETE | /api/v2/objectives/bulk-delete | - | 未実装 |
| POST | /api/v2/objectives/restore | `goal restore` | 実装済み |
| POST | /api/v2/objectives/archive | `goal archive` | 実装済み |
| POST | /api/v2/objectives/unarchive | `goal unarchive` | 実装済み |
| GET | /api/v2/objectives/:id/ancestors | - | 未実装 |
| GET | /api/v2/objectives/:id/children | `goal children` | 実装済み |
| GET | /api/v2/objectives/:id/descendants | - | 未実装 |
| GET | /api/v2/objectives/:id/deliverable-descendants | - | 未実装 |
| GET | /api/v2/objectives/:id/subtree | `goal tree` / `goal siblings` | 実装済み |
| GET | /api/v2/objectives/:id/similar | - | 未実装 |
| POST | /api/v2/objectives/:id/parent | `goal move` | 実装済み |
| POST | /api/v2/objectives/:id/insert-root | - | 未実装 |
| POST | /api/v2/objectives/:id/duplicate | `goal duplicate` | 実装済み |
| GET | /api/v2/objectives/:id/ai-schedule | - | 未実装 |
| PUT | /api/v2/objectives/:id/ai-schedule | - | 未実装 |
| GET | /api/v2/organizations/:id/objectives/editable-picker-tree | - | 未実装 |
| GET | /api/v2/organizations/:id/objectives/manager-inbox | - | 未実装 |
| GET | /api/v2/organizations/:id/objectives/:goalId/movement-summary | - | 未実装 |
| POST | /api/v2/organizations/:id/objectives/:goalId/manager-events | - | 未実装 |
| GET | /api/v2/objectives/:id/kpis | - | 未実装（`kpi list`相当なし） |
| POST | /api/v2/objectives/:id/kpis | `kpi add` | 実装済み |
| PATCH | /api/v2/objective-kpis/:id | `kpi update` | 実装済み |
| DELETE | /api/v2/objective-kpis/:id | `kpi rm` | 実装済み |
| GET | /api/v2/objective-kpis/:id/records | - | 未実装 |
| GET | /api/v2/objectives/:id/suggested-assignees | - | 未実装 |
| GET | /api/v2/objectives/:id/subtree/recurring-goals | - | 未実装 |
| GET/POST/PUT/DELETE | /api/v2/objectives/:id/recurring（4本） | - | 未実装 |
| GET | /api/v2/organizations/:id/recurring-goals | - | 未実装 |
| POST | /api/v2/objective/create | - | 未実装（実行タブ簡易作成、v2 `goal create`とは別実装） |
| POST | /api/v2/validate/objective/create | - | 対象外（検証用並走ルート） |
| GET/POST/DELETE | /api/v2/objectives/:id/slack-bindings（3本） | - | 未実装 |
| GET/PUT | /api/v2/objectives/:id/ai-followup-schedule（2本） | - | 未実装 |
| POST | /api/v1・v2/objectives/:id/assign-ai-agent | - | 未実装 |

### 9-2. AIバックグラウンドタスク（23機能×v1/v2重複=46本）

全23種（health-check, goal-tree, completed-review, bulk-decompose, create-plan, code-research, comment-summary, web-research, discussion-to-deliverable, schedule-check, recursive-assign, decompose, overdue-goals, duplicate-detection, progress-report, kpi-review, activity-analysis, workload-analysis, pr-review, issue-to-goal, organize-knowledge, bulk-reminders, auto-assign）: **すべて未実装**（46本）。Phase 3として計画書にも明記された大型未着手領域。

### 9-3. 割り当て (Assignment, 6本)

| Method | Path | CLIコマンド | 状態 |
|---|---|---|---|
| GET | /api/v2/objectives/:id/assignments | - | 未実装（`assignment list`相当なし） |
| POST | /api/v2/objectives/:id/assignments | `assignment add` | 実装済み |
| GET | /api/v2/objectives/:id/assignments/:assignmentId | - | 未実装 |
| PATCH | /api/v2/objectives/:id/assignments/:assignmentId | `assignment update` | 実装済み |
| DELETE | /api/v2/objectives/:id/assignments/:assignmentId | `assignment rm` | 実装済み |
| PUT | /api/v2/objectives/:id/transfer-ownership | `assignment transfer` | 実装済み |

### 9-4. Google Sheets紐付け（7本）

list / create / delete / service-account-info / create-for-service-account / trigger-report / update-schedule: **すべて未実装**。

## 10. 成果物 (Deliverable)

| Method | Path | CLIコマンド | 状態 |
|---|---|---|---|
| GET | /api/v1/team/objectives/:id/deliverables | `deliverable list` | 実装済み |
| POST | /api/v1/team/objectives/:id/deliverables | `deliverable add`（document/link/file） | 実装済み |
| GET | /api/v1/team/objectives/:id/deliverables/:deliverableId | - | 未実装（`deliverable get`相当なし） |
| POST | /api/v1/team/objectives/:id/deliverables/upload-complete/:deliverableId | `deliverable add --file`内部処理 | 実装済み |
| PATCH | /api/v1/team/objectives/:id/deliverables/:deliverableId | `deliverable update` | 実装済み |
| DELETE | /api/v1/team/objectives/:id/deliverables/:deliverableId | `deliverable rm` | 実装済み |
| PATCH | .../rename | `deliverable rename` | 実装済み |
| PATCH | .../move | `deliverable move` | 実装済み |
| POST | .../batch_move | `deliverable batch-move` | 実装済み |
| POST | .../batch_delete | `deliverable batch-rm` | 実装済み |
| GET | /api/v1/team/deliverables/:deliverableId | - | 未実装（フラットルート、通知起点の直接アクセス用） |

補足: `deliverable add` にフォルダタイプの明示フラグがなく、`create_folder_deliverable`（`POST .../deliverables`のFolder種別）はCLIから未使用（TUI専用実装が存在するのみ）。

## 11. コメント (Comment v1 DEPRECATED) / Goal Issue (v2) / 組織チャット (Org Chat)

### 11-1. コメント (v1 DEPRECATED chat-v1)

| Method | Path | CLIコマンド | 状態 |
|---|---|---|---|
| GET | /api/v1/team/comments（グローバル一覧） | - | 未実装 |
| GET | /api/v1/team/comments/:id/context | - | 未実装 |
| GET | /api/v1/team/comments/:id | `comment get` | 実装済み |
| POST | /api/v1/team/comments | `comment create` | 実装済み |
| PUT | /api/v1/team/comments/:id | `comment update` | 実装済み |
| DELETE | /api/v1/team/comments/:id | `comment delete` | 実装済み |
| DELETE | /api/v1/team/comments/:id/attachments/:attachmentId | `comment attachment rm` | 実装済み |
| PATCH | /api/v1/team/comments/:id/resolve | `comment resolve` | 実装済み |
| PATCH | /api/v1/team/comments/:id/unresolve | `comment unresolve` | 実装済み |
| POST | /api/v1/team/comments/:id/reactions | `comment react` | 実装済み |
| GET | /api/v1/team/comments/:id/reactions/:emoji/users | - | 未実装 |
| GET | /api/v2/objectives/:id/comments | `comment list` | 実装済み |

### 11-2. Goal Issue（v2チャット型、後継の本流 — 現状CLIは旧comment系に依存）

| 対象 | 本数 | 状態 |
|---|---|---|
| issues一覧/作成/編集/既読化、issueメッセージCRUD＋リアクション、全issue一覧/検索/プレビュー/解決状態設定、goal-sections（一覧/ピン留め/未読数） | 20本 | **すべて未実装** |

**注意**: comment系（chat-v1）はDEPRECATEDであり、goal-issue/goal-sectionsが後継の本流実装。現状CLIのコメント機能はレガシーAPIにのみ依存しているため、フロントエンドの実挙動との乖離が今後拡大するリスクがある（優先実装候補）。

### 11-3. 組織チャット (Org Chat)

| 対象 | 本数 | 状態 |
|---|---|---|
| ルーム/メッセージ/DM/グループ/招待/リアクション/既読等のCRUD一式 | 22本 | 未実装 |
| `/api/v2/chat/ws`（WebSocket） | 1本 | 対象外（内部/デバッグ用: WebSocketプロトコル） |
| `/api/v2/chat/admin/rooms`（+`/export`） | 2本 | 対象外（内部/デバッグ用: フラグ限定の内部監査エンドポイント） |

## 12. AIスレッド・エージェント (Thread / AI Agent / Agent Memory / Token Usage / Plan Subscription / Onboarding)

| 対象 | 本数 | 状態 |
|---|---|---|
| AIスレッドCRUD/チャット/トレース/共有/質問応答/ツール確認応答 | 16本 | 未実装 |
| トークン使用量・クォータ | 4本 | 未実装 |
| `POST /api/v1/team/ai/usage-ingest`（service認証） | 1本 | 対象外（内部/デバッグ用: サービス間認証専用） |
| `GET /api/v1/team/ai/token-balance`（service認証） | 1本 | 対象外（内部/デバッグ用: サービス間認証専用） |
| AIプランサブスクリプション（options/current/register/cancel/change/mode/downgrade取消/trial終了、Polar系9本） | 16本 | 未実装 |
| オンボーディング面談/アップロード/完了 | 3本 | 未実装 |
| Addness質問取得 | 1本 | 未実装 |
| AIエージェントCRUD/アバター/記憶/スキル紐付け/活動ログ/クレデンシャル/使用量/委任 | 13本 | 未実装 |

**小計: 53本未実装、2本対象外。** 現状CLIにAI関連コマンドは一切存在しない最大の未着手領域。

## 13. AIエージェントチャット (Goal Chat / Todo Chat / Core Values / Master Plan) / Goal Decompose

| 対象 | 本数 | 状態 |
|---|---|---|
| goal-chat（stream/encouragement/threads/messages） | 4本 | 未実装 |
| todo-chat（stream/threads/messages） | 3本 | 未実装 |
| todo-chat/validate/*（同上の検証用並走） | 3本 | 対象外（検証用並走ルート） |
| core-values（stream/threads/messages） | 3本 | 未実装 |
| master-plan（stream/threads/messages） | 3本 | 未実装 |

いずれもSSEストリーミング。CLIで実装する場合はストリーミング出力のUX設計が必要。

## 14. 通知 (Notification) / 通知設定 / プッシュトークン / メール宛先

| 対象 | 本数 | 状態 |
|---|---|---|
| 通知設定一覧/作成/更新、メール宛先一覧 | 4本 | 未実装 |
| 通知一覧/既読化/未読化/全既読化/未読数/目標別未読数/SSEストリーム | 7本 | 未実装 |

**注意**: `addness notification send` は実際には `POST /api/v1/team/comments`（コメント作成）を呼んでおり、本グループのどのエンドポイントとも対応しない。通知の閲覧・既読管理はCLIに一切存在せず、名前と実態が乖離している点は利用者の混乱要因。

## 15. 実行タブ・カレンダー (Goal Execution)

| Method | Path | CLIコマンド | 状態 |
|---|---|---|---|
| GET | /api/v2/personal/today-list | - | 未実装 |
| GET | /api/v2/personal/daily-activity | - | 未実装 |
| GET | /api/v2/codex/todays-goals/view | - | 未実装 |
| POST | /api/v2/codex/todays-goals/apply | - | 未実装 |
| GET | /api/v2/organizations/:id/todays-goals/summary | - | 未実装 |
| GET | /api/v2/organizations/:id/todays-goals | `today` / `today list` | 実装済み |
| GET/POST/PATCH/DELETE | /api/v2/organizations/:id/today-todos（+activities）（5本） | - | 未実装 |
| GET/POST/PATCH/DELETE/POST | /api/v2/organizations/:id/planned-todos（+material, +adopt）（6本） | - | 未実装 |
| GET/POST | /api/v2/organizations/:id/calendar-events（+completion）（2本） | - | 未実装 |
| GET | /api/v2/organizations/:id/goal-calendar | - | 未実装 |
| GET | /api/v2/organizations/:id/goal-history | - | 未実装 |
| GET | /api/v2/organizations/:id/execute-goals/summary | - | 未実装 |
| GET/PUT | /api/v2/organizations/:id/preferences/goal-collapse（2本） | - | 未実装 |
| POST/PUT/GET | /api/v2/execute-goals/generate, /:id, /history（3本） | - | 未実装 |
| GET | /api/v2/todays-goals/active-huddles | - | 未実装 |
| GET/POST/PATCH/DELETE | /api/v2/organizations/:id/validate/...（18本） | - | 対象外（検証用並走ルート） |
| GET/POST | /api/v2/personal/validate/today-list, /daily-activity（2本） | - | 対象外（検証用並走ルート） |
| GET/POST | /api/v2/codex/validate/todays-goals/view, /apply（2本） | - | 対象外（検証用並走ルート） |

**注意**: `today done` / `today reopen` / `today status` は今日のゴール一覧に表示されるゴール自体の状態を`PATCH /api/v2/objectives/{id}`（グループ9）で更新しているだけで、本グループの`today-todos`（chat由来の一時ToDo行）とは別物。今日のToDo・計画中ToDo・カレンダー・履歴・実行計画生成は未実装。

## 16. アクティビティログ (Activity Log)

メンバー別/ゴール別/サマリ（v1）、ゴールサマリ（v2）: 全4本 **未実装**。

## 17. ミーティング (Huddle音声通話 / Meeting Bot / Meeting Note・Minutes)

Huddle参加/退出/切替/状態/録音/画面共有/招待（16本）、Meeting Bot（Recall.ai）ジョブ管理（4本）、Meeting Note文字起こし/要約/議事録投稿/ゴール提案/ゴール作成（5本）、議事録CRUD（5本）: 全30本 **未実装**。音声/動画/録画を扱うため、CLIでの実装優先度は低い。

## 18. ストリーク (Streak)

共有状態取得/共有リンク作成/失効、フリーズ/解除、復活、ストリーク取得、公開共有取得: 全8本 **未実装**。ゲーミフィケーション要素で業務上の優先度は低い。

## 19. スキル / ツール (Skill / Tool)

スキルCRUD/検索/パフォーマンス/リソースCRUD/改善提案の承認・却下（15本）、ツールCRUD/検索/実行（5本）: 全20本 **未実装**。

## 20. 個人スペース (Personal)

現在時刻/今日ドキュメント取得・追記/テキストパッチ/Markdown操作各種（analyze/replace-section/upsert-section/upsert-list-item/replace-document/append-log-entry）/日別エントリ/エージェントセッションCRUD/プロジェクトCRUD/リセット、個人組織ensure: 全21本 **未実装**。

## 21. 検索 / 診断 / 紹介 / 請求書 / ゴールツリー共有 / インラインメディア

| 対象 | 本数 | 状態 |
|---|---|---|
| 統合検索 | 1本 | 未実装 |
| 診断結果（保存/一覧/種別取得/統計/公開設定/メンバープロフィール一覧・取得） | 8本 | 未実装 |
| 紹介実績/コンバージョン記録 | 2本 | 未実装 |
| `GET /api/v1/admin/referrals/logs` | 1本 | 対象外（admin専用） |
| 請求書一覧 | 1本 | 未実装 |
| 公開共有（目標/ゴールツリー/AIスレッド、いずれも認証不要） | 3本 | 未実装 |
| ゴールツリー共有（作成/失効/自分の一覧/クローン） | 4本 | 未実装 |
| インラインメディア表示/アップロードURL発行 | 2本 | 未実装 |
| ゴール活動レポートスケジュール（取得/更新/削除/テスト送信） | 4本 | 未実装（うち`report-schedule/test`はリリース前削除予定の内部検証用） |

## 22. Codexジョブ (v1 / v2)

一覧/詳細/作成/入力送信/再開/クローズ/キャンセル/イベントストリーム（v1: 8本、v2: 8本、v2のみ削除あり）: 全16本 **未実装**。Codex CLI/エージェント側から直接利用されるため、Addness CLIからの実装優先度は要検討。

## 23. 管理者 (Admin)

全5本: **対象外（admin専用）**。組織ダッシュボード、サブスク強制更新、AIプランサブスク一覧、全体トークン使用量、紹介ログ — いずれも社内オペレーション専用。

## 24. ALB用エイリアスルート

全7本: **対象外（ALBエイリアス）**。`/api/v1/team/ai/threads/*`・`/api/v2/organizations/:id/ai-agents*`の別名ルートで実体は同一。

---

## 対象外リスト（カテゴリ別）

### 1. Webhook / 外部コールバック受信専用（計23本）
- グループ3 Webhook 全9本
- グループ2: `GET /api/v1/auth/google/callback`
- グループ4: `GET /api/v1/slack/oauth/callback`, `POST /api/v1/slack/commands/notify`, `GET /api/v1/discord/oauth/callback`, `POST /api/v1/discord/link-channel`, `GET /api/v1/team/zoom/oauth/callback`, `GET /api/v1/github/callback`, `GET /api/v2/codex/integrations/oauth/callback`, `GET /api/v2/codex/integrations/slack/callback`, `GET /api/v2/codex/integrations/github/callback`

### 2. 内部/デバッグ用（計16本）
- グループ1 全2本（ヘルスチェック）
- グループ2: `.well-known/*`（4本）, `/authorize`, `/register`, `/token`, `mcp-oauth/code`, `mcp/uploads/:ticket`（計9本）
- グループ5 MCPプロトコル全3本
- グループ11: `/api/v2/chat/ws`, `/api/v2/chat/admin/rooms`, `/api/v2/chat/admin/rooms/:roomId/export`（3本）
- グループ12: `/api/v1/team/ai/usage-ingest`, `/api/v1/team/ai/token-balance`（service認証専用、2本）— ※上記合計に含む場合は21本

### 3. 検証用並走ルート（計47本）
- グループ9: `POST /api/v2/validate/objective/create`（1本）
- グループ13: `ai-todo-chat/validate/*`（3本）
- グループ15: `organizations/:id/validate/...`（18本）, `personal/validate/*`（2本）, `codex/validate/*`（2本）

### 4. ALBエイリアス（計7本）
- グループ24 全7本

### 5. admin専用（計6本）
- グループ23 全5本
- グループ21: `GET /api/v1/admin/referrals/logs`（グループ23と同一エンドポイントが2箇所のルートグループに重複登録されているため別カウント）

### 6. v1レガシー重複（v2で代替実装済み、計6本）
- グループ7: `GET /api/v1/team/organizations`, `GET /api/v1/team/organizations/:id/organization_members`
- グループ8: `POST /api/v1/team/organization_invitations`
- グループ9: `GET /api/v1/team/objectives/:id`, `GET /api/v1/team/objectives/:id/children`, `POST /api/v1/team/objectives`, `PATCH /api/v1/team/objectives/:id`, `PATCH /api/v1/team/objectives/:id/change_parent`

---

## 未実装エンドポイントの実装優先順位（推奨）

「依存が少なく（外部OAuth・SSEストリーミング・音声/録画等の複雑な状態管理が不要）、利用頻度が高い（既存CLIワークフローの中核機能を補完する）」ものから着手する。

### Tier 1（最優先: 既存機能の欠けている読み取り操作・自動化に必須な基本機能）

1. **APIキー管理**（`POST/GET/DELETE /api/v1/team/api-keys`）— CI/自動化パイプラインでの認証に必須。単純なCRUDで依存なし。
2. **KPI一覧取得**（`GET /api/v2/objectives/:id/kpis`, `GET /api/v2/objective-kpis/:id/records`）— `kpi add/update/rm`はあるのに`list`が無い非対称を解消。
3. **Assignment一覧・詳細取得**（`GET /api/v2/objectives/:id/assignments`, `GET .../assignments/:assignmentId`）— 同上の非対称解消。
4. **ゴール祖先/子孫取得**（`GET /api/v2/objectives/:id/ancestors`, `/descendants`）— `goal tree`/`goal children`と並ぶナビゲーション基本機能で依存なし。
5. **自分のユーザー情報取得・更新**（`GET/PUT /api/v1/team/users/current`系, `user_settings`）— プロフィール確認は利用頻度が高い基本機能。
6. **通知一覧・既読管理**（`GET/POST /api/v2/organizations/:id/notifications*`）— `notification send`が実は通知APIを叩いていない非対称を解消。単純なCRUD。
7. **Goal Issue基本機能**（issue一覧/作成/メッセージ投稿、`GET/POST /api/v2/objectives/:id/issues*`）— comment系(v1 DEPRECATED)にCLIが依存し続けているリスクを解消する本流実装。

### Tier 2（中頻度・中依存: 既存ワークフローの拡張）

8. Recurring Goal CRUD（`GET/POST/PUT/DELETE /api/v2/objectives/:id/recurring`）
9. Goal text-patch（`POST /api/v2/objectives/:id/text-patch`）— 既知のギャップとして計画書に明記済み
10. 招待の個人視点コマンド（`invitations/pending`, `decline`, `:token/accept`）— 招待「受け取る側」の操作が現状ゼロ
11. Slack/Sheetsバインディング（`objectives/:id/slack-bindings`, `sheet-bindings`）
12. 今日のToDo・計画中ToDo・カレンダー（`today-todos`, `planned-todos`, `calendar-events`, `goal-calendar`, `goal-history`）— `today`コマンドの自然な拡張
13. アクティビティログ（`activity-logs/*`）
14. Memberの残り（search, children, avatar, tags, delete-preview）
15. 組織詳細取得・タイムゾーン設定（`GET /organizations/:id`, `default-timezone`）

### Tier 3（低頻度・高依存: 外部サービス連携やストリーミングが絡み実装コストが高い）

16. 外部連携（Slack/Discord/GitHub/LINE/Zoom/Codex Integrations）— OAuthブラウザ導線が前提でCLI単体では完結しない
17. AIスレッド/エージェント/AIバックグラウンドタスク（Phase 3全般）— レート制限・SSEストリーミング・複雑な状態遷移
18. AIエージェントチャット（goal-chat/todo-chat/core-values/master-plan）— SSEストリーミング
19. ミーティング（Huddle/Meeting Bot/Minutes）— 音声/録画/LiveKitトークンなどCLIに不向き
20. 個人スペース（Personal）— 既存の`today`コマンド群と機能が重複気味で、Markdown編集の細粒度APIをCLIに露出する必要性が低い
21. スキル/ツール管理 — 組織単位の高度な設定で利用頻度が低い
22. ストリーク — ゲーミフィケーション要素で業務優先度が低い
23. Codexジョブ — Codex CLI/エージェント側の専用ワークフローで、Addness CLIからの必要性は限定的
24. 診断/紹介/請求書/ゴールツリー共有等 — 低頻度な参照・共有機能

---

関連ドキュメント: [`docs/backend-endpoint-inventory.md`](./backend-endpoint-inventory.md)（本表の元になったバックエンド659ルート棚卸し表）
