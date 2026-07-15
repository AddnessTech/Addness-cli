# バックエンド（vision-todo-backend）エンドポイント棚卸し表

- 対象: `vision-todo-backend`（Go / Gin, main ブランチの読み取り専用調査用worktree断面）
- ルート登録箇所: `presentation/routes/api.go`（メイン, 2000行超）, `presentation/routes/mcp_route.go`（MCPプロトコル用汎用ハンドラ登録）
- ベースパスグループ:
  - `/api/v1`（レガシー）
  - `/api/v2`（DDD新設計、本流）
  - `/api/ai/v1` `/api/ai/v2`（ALBパスマッチング段階移行用エイリアス。ハンドラは`/api/v1`・`/api/v2`と共用）
  - ルート直下（`/mcp*`, `/.well-known/*`, `/authorize`, `/register`, `/token`）
- 総エンドポイント登録数（`r.GET/POST/PUT/PATCH/DELETE/Any(...)` 直接呼び出し + `postJSON/patchJSON/putJSON/deleteJSON/*NoContent` ヘルパー経由の登録を機械的に集計、`registerMCPRoute`のAny(3本)含む）: **659本**
  - v1/v2/validate/ALBの重複カウント方法の違いにより、下記グループ別サマリの内訳合計と若干のズレがあるが、両者とも「実装上の登録個数」を指す。パスパターンの重複を除いた実質ユニークURL数は概ね450〜480程度と推定される。
- 認証方式の凡例:
  | 表記 | 意味 |
  |---|---|
  | `Clerk` | Clerk JWT必須 |
  | `Clerk/APIKey` | `apiKeyAuthMiddleware.Middleware(clerkAuthMiddleware.Middleware())` — どちらかで認証 |
  | `+Sub` | 上記に加え `SubscriptionGuard`（課金ガード）必須 |
  | `+Org` | `OrganizationMemberGuard`（組織所属検証のみ、課金不問） |
  | `+Admin` | 組織admin権限必須（UseCase内 or ミドルウェアで検証） |
  | `個人スコープ` | APIKey利用時は `ApiKeyScopePersonal` 必須 |
  | `service` | `X-Addness-App-Key` によるサービス間認証 |
  | `不要` | 認証なし（署名/state/トークン/HMAC等で別途検証するものを含む） |
- 「備考」列に internal/debug/webhook/検証用並走ルート/ALBエイリアス等の特記事項を記載している。これらの分類は `docs/cli-endpoint-coverage.md` の「対象外」定義と対応する。
- 本表はCLI実装計画（`docs/cli-endpoint-coverage.md`）の土台となる棚卸し表であり、バックエンドコード自体は変更していない。

---

## 1. システム / ヘルスチェック

| Method | Path | Handler | 認証 | 用途 |
|---|---|---|---|---|
| GET | /api/v1/health | healthHandler | 不要 | ヘルスチェック |
| GET | /api/v1/health/ready | healthReadyHandler | 不要 | readinessチェック（DB接続確認） |

## 2. 認証・MCP OAuth・APIキー・デスクトップ認証

| Method | Path | Handler | 認証 | 用途 | 備考 |
|---|---|---|---|---|---|
| GET | /.well-known/oauth-protected-resource | mcpOAuthServer.ProtectedResource | 不要 | MCP OAuth protected resource metadata | mcpOAuthServer設定時のみ登録 |
| GET | /.well-known/oauth-protected-resource/*path | mcpOAuthServer.ProtectedResource | 不要 | 同上（パス付き） | 同上 |
| GET | /.well-known/oauth-authorization-server | mcpOAuthServer.AuthServerMetadata | 不要 | MCP OAuth認可サーバメタデータ | 同上 |
| GET | /.well-known/oauth-authorization-server/*path | mcpOAuthServer.AuthServerMetadata | 不要 | 同上（パス付き） | 同上 |
| GET | /authorize | mcpOAuthServer.Authorize | 不要（OAuthフロー） | MCP OAuth認可エンドポイント | 同上 |
| POST | /register | mcpOAuthServer.Register | 不要 | MCP OAuth動的クライアント登録 | 同上 |
| POST | /token | mcpOAuthServer.Token | 不要（ボディ内クライアント認証） | MCP OAuthトークン発行 | 同上 |
| POST | /api/v1/team/mcp-oauth/code | mcpOAuthServer.GenerateCode | Clerk/APIKey | MCP OAuth認可コード生成（認証済みユーザー向け） | 同上 |
| POST | /api/v1/mcp/uploads/:ticket | deliverableHandler.ProxyUpload | 不要（URL内HMACチケット） | MCPデリバラブルのプロキシアップロード | Bearer/Clerk認証グループの外（rawエンジン）に配置 |
| POST | /api/v1/team/api-keys | apiKeyHandler.Create | Clerk/APIKey | APIキー発行 | |
| GET | /api/v1/team/api-keys | apiKeyHandler.List | Clerk/APIKey | APIキー一覧 | |
| DELETE | /api/v1/team/api-keys/:id | apiKeyHandler.Revoke | Clerk/APIKey | APIキー失効 | |
| GET | /api/v1/auth/google/callback | externalOAuthHandler.GoogleCallback | 不要（Cookie+state検証） | Google OAuthコールバック（Calendar/Drive等） | |
| POST | /api/v1/public/desktop/auth/installations/register | desktopAuthHandler.RegisterInstallation | 不要（IPレート制限） | デスクトップアプリのインストール登録 | |
| POST | /api/v1/public/desktop/auth/start-sessions | desktopAuthHandler.CreateStartSession | 不要（IPレート制限） | デスクトップ認証開始セッション作成 | |
| POST | /api/v1/public/desktop/auth/start-sessions/redeem | desktopAuthHandler.RedeemStartSession | 不要（IPレート制限） | 開始セッションの引き換え | |
| POST | /api/v1/public/desktop/auth/token-exchange | desktopAuthHandler.ExchangeToken | 不要（IPレート制限） | デスクトップ認証トークン交換 | |
| POST | /api/v1/team/desktop/auth/intents/:id/complete | desktopAuthHandler.CompleteIntent | Clerk/APIKey | デスクトップ認証インテント完了 | |
| GET | /api/v2/me/consents/:consentType | userConsentHandler.GetConsent | Clerk/APIKey（個人スコープ） | 法的同意状態取得（チャットの通信の秘密） | |
| POST | /api/v2/me/consents | userConsentHandler.RecordConsent | Clerk専用（APIKey不可） | 法的同意の本人記録 | |

## 3. Webhook（すべて認証不要・署名/トークンで検証）

| Method | Path | Handler | 用途 | 備考 |
|---|---|---|---|---|
| POST | /api/v1/webhooks/univapay | univapayWebhookHandler.HandleUnivapayWebhook | Univapay決済webhook | |
| POST | /api/v1/webhooks/polar | polarWebhookHandler.HandlePolarWebhook | Polar課金webhook | |
| POST | /api/v1/webhooks/clerk | clerkWebhookHandler.HandleClerkWebhook | Clerk認証webhook（ユーザー作成等） | |
| POST | /api/v1/webhooks/github | githubWebhookHandler.HandleWebhook | GitHub Appイベントwebhook | |
| POST | /api/v1/webhooks/zoom | zoomHandler.HandleWebhook | Zoomイベントwebhook | |
| POST | /api/v1/webhooks/line | lineComponents.WebhookHandler.HandleWebhook | LINE Messaging API webhook | LINE有効時のみ登録 |
| POST | /api/v1/webhooks/livekit | huddleComponents.WebhookHandler.Handle | LiveKit（音声通話）イベントwebhook | Huddle有効時のみ登録 |
| POST | /api/v1/webhooks/recall | meetingBotComponents.WebhookHandler.Handle | Recall.ai（議事録Bot）イベントwebhook | MeetingBot有効時のみ登録。内部/デバッグ寄りの内部連携webhook |
| POST | /api/v1/webhooks/google-drive | sheetsWatchComponents.WebhookHandler.HandleDriveNotification（inline func経由） | Google Drive Push通知receiver | X-Goog-Channel-Tokenで識別。SA鍵未設定時はログのみ返しStatus 200 |

## 4. 外部連携（Slack / Discord / GitHub / Google / LINE / Zoom / Codex Integrations）

| Method | Path | Handler | 認証 | 用途 | 備考 |
|---|---|---|---|---|---|
| GET | /api/v1/slack/oauth/callback | slackHandler.OAuthCallback | 不要（OAuth state検証） | Slack OAuthコールバック | |
| POST | /api/v1/slack/commands/notify | slackHandler.SlashNotify | 不要（Slack署名検証） | Slackスラッシュコマンド | |
| GET | /api/v1/discord/oauth/callback | discordHandler.OAuthCallback | 不要（state検証） | Discord OAuthコールバック | |
| POST | /api/v1/discord/link-channel | discordLinkChannelHandler.LinkChannel | 不要（リンクコード認証） | Discord botからのチャンネルリンクリクエスト | |
| GET | /api/v1/team/zoom/oauth/callback | zoomHandler.Callback | 不要（state検証） | Zoom OAuthコールバック | |
| GET | /api/v1/github/callback | githubCallbackHandler.Callback | 不要 | GitHub App連携コールバック | |
| GET | /api/v2/codex/integrations/oauth/callback | codexComponents.Handler.HandleIntegrationOAuthCallback | 不要（サーバ側state/nonce+PKCE） | Codex連携 OAuth Connectコールバック | ルートエンジン直下の固定パス（route-check静的解決のためリテラル登録） |
| GET | /api/v2/codex/integrations/slack/callback | codexComponents.Handler.HandleSlackInstallCallback | 不要 | codex専用SlackアプリのOAuth installコールバック | 同上 |
| GET | /api/v2/codex/integrations/github/callback | codexComponents.Handler.HandleGitHubInstallCallback | 不要 | codex専用GitHub AppのOAuth installコールバック | 同上 |
| GET | /api/v1/team/integrations/slack/connect | slackInstallationHandler.Connect | Clerk/APIKey | Slack OAuth接続開始 | |
| GET | /api/v1/team/integrations/slack/installations | slackInstallationHandler.ListInstallations | Clerk/APIKey | Slackワークスペース一覧（マルチワークスペース対応） | |
| DELETE | /api/v1/team/integrations/slack/installations/:installationId | slackInstallationHandler.DeleteInstallation | Clerk/APIKey | Slack連携解除（workspace個別） | |
| GET | /api/v1/team/integrations/slack/destinations | slackDestinationHandler.ListDestinations | Clerk/APIKey | Slack通知先一覧 | |
| GET | /api/v1/team/integrations/slack/channels | slackInstallationHandler.ListChannels | Clerk/APIKey | Slackチャンネル一覧 | |
| GET | /api/v1/team/integrations/slack/channels/:channelId/history | slackInstallationHandler.ReadChannelHistory | Clerk/APIKey | Slackチャンネル履歴読み取り（AIチャット用） | |
| POST | /api/v1/team/integrations/slack/messages | slackInstallationHandler.PostMessage | Clerk/APIKey | Slackメッセージ投稿 | |
| GET | /api/v1/team/integrations/discord/connect | discordInstallationHandler.Connect | Clerk/APIKey | Discord OAuth接続開始 | |
| GET | /api/v1/team/integrations/discord/installations | discordInstallationHandler.ListInstallations | Clerk/APIKey | Discordサーバー一覧 | |
| DELETE | /api/v1/team/integrations/discord/installations/:installationId | discordInstallationHandler.DeleteInstallation | Clerk/APIKey | Discord連携解除 | |
| GET | /api/v1/team/integrations/discord/installations/:installationId/channels | discordDestinationHandler.ListChannels | Clerk/APIKey | Discordチャンネル一覧 | |
| PUT | /api/v1/team/integrations/discord/destinations | discordDestinationHandler.UpsertDestination | Clerk/APIKey | Discord通知先設定 | |
| GET | /api/v1/team/integrations/discord/destinations | discordDestinationHandler.ListDestinations | Clerk/APIKey | Discord通知先一覧 | |
| DELETE | /api/v1/team/integrations/discord/destinations/:destinationId | discordDestinationHandler.DeleteDestination | Clerk/APIKey | Discord通知先削除 | |
| GET | /api/v1/team/integrations/github/install | githubHandler.Install | Clerk/APIKey | GitHub Appインストール開始 | |
| GET | /api/v1/team/integrations/github/installation | githubHandler.GetInstallation | Clerk/APIKey | インストール情報取得 | |
| DELETE | /api/v1/team/integrations/github/installation | githubHandler.DeleteInstallation | Clerk/APIKey | インストール解除 | |
| GET | /api/v1/team/integrations/github/repos | githubHandler.ListRepos | Clerk/APIKey | リポジトリ一覧 | |
| PATCH | /api/v1/team/integrations/github/repos/:repoId | githubHandler.ToggleRepo | Clerk/APIKey | リポジトリ連携 有効/無効切替 | |
| GET | /api/v1/team/integrations/line/friend-url | lineComponents.LinkHandler.FriendURL | Clerk/APIKey | LINE友達追加URL取得 | LINE有効時のみ登録 |
| GET | /api/v1/team/integrations/line/link-info | lineComponents.LinkHandler.LinkInfo | Clerk/APIKey | リンク情報取得 | 同上 |
| POST | /api/v1/team/integrations/line/link | lineComponents.LinkHandler.LinkAccount | Clerk/APIKey | LINEアカウントリンク | 同上 |
| GET | /api/v1/team/integrations/line/status | lineComponents.LinkHandler.Status | Clerk/APIKey | リンク状態取得 | 同上 |
| DELETE | /api/v1/team/integrations/line/link | lineComponents.LinkHandler.Unlink | Clerk/APIKey | リンク解除 | 同上 |
| POST | /api/v1/team/integrations/credentials | externalCredentialHandler.Save | Clerk/APIKey | 外部連携クレデンシャル保存（BYOK） | |
| GET | /api/v1/team/integrations/credentials | externalCredentialHandler.List | Clerk/APIKey | クレデンシャル一覧 | |
| GET | /api/v1/team/integrations/credentials/:service | externalCredentialHandler.GetByService | Clerk/APIKey | サービス別クレデンシャル取得 | |
| DELETE | /api/v1/team/integrations/credentials/:service | externalCredentialHandler.DeleteByService | Clerk/APIKey | クレデンシャル削除 | |
| GET | /api/v1/team/integrations/google/connect | externalOAuthHandler.GoogleConnect | Clerk/APIKey | Google OAuth接続開始 | |
| DELETE | /api/v1/team/integrations/google/disconnect | externalOAuthHandler.GoogleDisconnect | Clerk/APIKey | Google連携解除 | |
| GET | /api/v1/team/integrations/google/picker-token | externalOAuthHandler.GooglePickerToken | Clerk/APIKey | Drive Picker JS API用の短命access token | |
| GET | /api/v1/team/zoom/status | zoomHandler.Status | Clerk/APIKey | Zoom連携状態取得 | |
| DELETE | /api/v1/team/zoom/disconnect | zoomHandler.Disconnect | Clerk/APIKey | Zoom連携解除 | |
| GET | /api/v1/team/zoom/auth/start | zoomHandler.StartAuth | Clerk/APIKey | Zoom OAuth開始 | |
| GET | /api/v1/team/zoom/jobs | zoomHandler.ListActiveJobs | Clerk/APIKey | アクティブなジョブ一覧 | |
| POST | /api/v1/team/zoom/jobs | zoomHandler.CreateJob | Clerk/APIKey | ジョブ作成（録画取込） | |
| GET | /api/v1/team/zoom/jobs/:id | zoomHandler.JobStatus | Clerk/APIKey | ジョブステータス取得 | |
| DELETE | /api/v1/team/zoom/jobs/:id | zoomHandler.DeleteJob | Clerk/APIKey | ジョブ削除 | |
| GET | /api/v1/team/zoom/jobs/:id/stream | zoomHandler.JobStatusStream | Clerk/APIKey | ジョブステータスSSEストリーム | |
| GET | /api/v1/team/zoom/jobs/:id/summary | zoomHandler.FetchSummary | Clerk/APIKey | ジョブサマリ取得 | |
| POST | /api/v1/team/link-codes/slack | linkCodeHandler.IssueSlackLinkCode | Clerk/APIKey | Slackリンクコード発行 | |
| POST | /api/v1/team/link-codes/discord | linkCodeHandler.IssueDiscordLinkCode | Clerk/APIKey | Discordリンクコード発行 | |
| GET | /api/v2/codex/integrations | codexComponents.Handler.ListIntegrations | Clerk/APIKey | 連携マーケットプレイス一覧（org単位MCP設定） | |
| PUT | /api/v2/codex/integrations/:name | codexComponents.Handler.ConnectIntegration | Clerk/APIKey | 連携接続 | |
| DELETE | /api/v2/codex/integrations/:name | codexComponents.Handler.DisconnectIntegration | Clerk/APIKey | 連携切断 | |
| POST | /api/v2/codex/integrations/:name/oauth/start | codexComponents.Handler.StartIntegrationOAuth | Clerk/APIKey | 連携OAuth開始 | |
| GET | /api/v2/codex/integrations/slack/connect | codexComponents.Handler.SlackConnectStart | Clerk/APIKey | codex専用Slack OAuth接続開始 | 静的パス優先でGETツリーの:name動的パスと共存 |
| GET | /api/v2/codex/integrations/github/connect | codexComponents.Handler.GitHubConnectStart | Clerk/APIKey | codex専用GitHub OAuth接続開始 | 同上 |

## 5. MCPプロトコル（内部/デバッグ寄り）

| Method | Path | Handler | 認証 | 用途 | 備考 |
|---|---|---|---|---|---|
| ANY(GET/POST/DELETE/OPTIONS) | /mcp | mcpHTTPServer | Bearer（MCP内部でトークン検証） | MCP Streamable HTTPサーバー（通常ゴール操作ツール） | 内部プロトコルエンドポイント。認証失敗時はOAuth challengeへ誘導 |
| ANY | /mcp/codex | mcpCodexHTTPServer | Bearer（MCP内部検証） | MCP Streamable HTTPサーバー（Codex専用） | 同上 |
| ANY | /mcp/personal | mcpPersonalHTTPServer | Bearer（MCP内部検証） | MCP Streamable HTTPサーバー（個人スコープ） | 同上 |

## 6. ユーザー / ユーザー設定

| Method | Path | Handler | 認証 | 用途 |
|---|---|---|---|---|
| GET | /api/v1/team/users/current | userHandler.GetCurrentUser | Clerk/APIKey | 自分のユーザー情報取得 |
| PUT | /api/v1/team/users/:id | userHandler.Update | Clerk/APIKey | ユーザー更新 |
| GET | /api/v1/team/user_settings | userHandler.GetUserSettings | Clerk/APIKey | ユーザー設定取得 |
| PATCH | /api/v1/team/user_settings | userHandler.UpdateUserSettings | Clerk/APIKey | ユーザー設定更新 |
| GET | /api/v1/team/organization_members | userHandler.ListOrganizationMembers | Clerk/APIKey | 組織メンバー一覧（認証ユーザー用、v1レガシー） |
| GET | /api/v1/team/users | userHandler.List | Clerk/APIKey+Sub | ユーザー一覧（v1） |
| GET | /api/v1/team/users/:id | userHandler.FindByID | Clerk/APIKey+Sub | ユーザー詳細（v1） |
| POST | /api/v1/team/users | userHandler.Create | Clerk/APIKey+Sub | ユーザー作成（v1） |
| DELETE | /api/v1/team/users/:id | userHandler.Delete | Clerk/APIKey+Sub | ユーザー削除（v1） |

## 7. 組織 (Organization)

| Method | Path | Handler | 認証 | 用途 |
|---|---|---|---|---|
| GET | /api/v1/team/organizations | organizationHandler.List | Clerk/APIKey | 組織一覧（v1） |
| POST | /api/v1/team/organizations | organizationHandler.Create | Clerk/APIKey | 組織作成（v1） |
| GET | /api/v1/team/organizations/:id | organizationHandler.FindByID | Clerk/APIKey | 組織詳細（v1） |
| DELETE | /api/v1/team/organizations/:id | organizationHandler.Delete | Clerk/APIKey | 組織削除（v1） |
| GET | /api/v1/team/organizations/:id/organization_members | organizationHandler.GetMembers | Clerk/APIKey | 組織メンバー一覧（v1） |
| GET | /api/v1/team/organizations/:id/root_owner | organizationHandler.GetRootOwner | Clerk/APIKey | ルートオーナー取得 |
| GET | /api/v1/team/organizations/:id/accessible_root | organizationHandler.GetAccessibleRoot | Clerk/APIKey | アクセス可能ルート取得 |
| GET | /api/v1/team/organizations/:id/ai_agent_member | organizationHandler.GetAIAgentMember | Clerk/APIKey | AIエージェントメンバー取得 |
| GET | /api/v1/team/organizations/:id/access-state | organizationHandler.GetAccessState | Clerk/APIKey | 課金アクセス状態取得 |
| POST | /api/v1/team/organizations/:id/push_tokens | pushTokenHandler.Register | Clerk/APIKey | プッシュ通知トークン登録 |
| POST | /api/v1/team/organization_subscriptions/register | organizationSubscriptionHandler.RegisterSubscription | Clerk/APIKey | サブスク登録 |
| PATCH | /api/v1/team/organization_subscriptions/:id/cancel | organizationSubscriptionHandler.CancelSubscription | Clerk/APIKey | サブスク解約 |
| GET | /api/v1/team/organization_subscriptions/current | organizationSubscriptionHandler.GetCurrentSubscription | Clerk/APIKey | 現行サブスク取得 |
| GET | /api/v2/organizations/me | organizationV2Handler.Me | Clerk/APIKey | 自分の所属組織一覧（組織検証不要） |
| GET | /api/v2/organizations | organizationV2Handler.List | Clerk/APIKey+Sub | 組織一覧（v2） |
| GET | /api/v2/organizations/:id/objectives/tree | objectiveV2Handler.GetOrganizationTree | Clerk/APIKey+Sub | 組織のゴールツリー取得 |
| PUT | /api/v2/organizations/:id/logo | organizationV2Handler.UploadLogo | Clerk/APIKey+Org+Admin | 組織ロゴアップロード |
| PUT/PATCH | /api/v2/organizations/:id | organizationV2Handler.Update | Clerk/APIKey+Org+Admin | 組織情報更新（名前等） |
| PUT/PATCH | /api/v2/organizations/:id/default-timezone | organizationV2Handler.UpdateDefaultTimezone | Clerk/APIKey+Org+Admin | 既定タイムゾーン更新 |
| GET | /api/v2/organizations/:id/context | organizationV2Handler.GetContext | Clerk/APIKey+Org | 組織コンテキスト取得（Addy向けCLAUDE.md相当） |
| PATCH | /api/v2/organizations/:id/context | organizationV2Handler.UpdateContext | Clerk/APIKey+Org+Admin | 組織コンテキスト更新 |
| GET | /api/v2/organizations/:id/context/revisions | organizationV2Handler.ListContextRevisions | Clerk/APIKey+Org | コンテキスト履歴一覧 |
| GET | /api/v2/organizations/:id/onboarding-billing-state | organizationV2Handler.GetOnboardingBillingState | Clerk/APIKey+Org | オンボーディング課金状態取得 |
| POST | /api/v2/organizations/:id/onboarding-billing/require | organizationV2Handler.RequireOnboardingBilling | Clerk/APIKey+Org | 課金必須化 |
| POST | /api/v2/organizations/:id/onboarding-billing/free | organizationV2Handler.CompleteOnboardingBillingFree | Clerk/APIKey+Org | 無料プランで完了 |
| GET | /api/v2/organizations/:id/ai-schedule-settings | objectiveV2Handler.GetAIScheduleSetting | Clerk/APIKey+Org+Admin | AIスケジュール組織単位マスタースイッチ取得 |
| PUT | /api/v2/organizations/:id/ai-schedule-settings | objectiveV2Handler.UpsertAIScheduleSetting | Clerk/APIKey+Org+Admin | 同上更新 |
| GET | /api/v2/organizations/:id/ad-settings | organizationV2Handler.GetAdSetting | Clerk/APIKey+Org | アプリ内広告設定取得（組織全体） |
| PUT | /api/v2/organizations/:id/ad-settings | organizationV2Handler.UpsertAdSetting | Clerk/APIKey+Org+Admin | アプリ内広告設定更新（組織全体） |
| GET | /api/v2/organizations/:id/ad-settings/me | organizationV2Handler.GetMyAdSetting | Clerk/APIKey+Org | アプリ内広告設定取得（本人） |
| PUT | /api/v2/organizations/:id/ad-settings/me | organizationV2Handler.UpsertMyAdSetting | Clerk/APIKey+Org | アプリ内広告設定更新（本人） |
| GET | /api/v2/organizations/:id/admin/check | memberV2Handler.CheckAdmin | Clerk/APIKey | callerがadminかチェック（自己検証） |
| GET | /api/v2/organizations/:id/current-member | memberV2Handler.CurrentMember | Clerk/APIKey | callerの当該組織メンバー情報取得（bootstrap/recovery用） |

## 8. メンバー (Member) / メンバータグ / 招待 (Invitation)

| Method | Path | Handler | 認証 | 用途 |
|---|---|---|---|---|
| GET | /api/v2/organizations/:id/members | memberV2Handler.List | Clerk/APIKey+Org | メンバー一覧（v2, Settings表示） |
| GET | /api/v2/organizations/:id/members/search | memberV2Handler.Search | Clerk/APIKey+Org | メンバー検索 |
| GET | /api/v2/organizations/:id/members/children | memberV2Handler.Children | Clerk/APIKey+Org | 子メンバー取得 |
| GET | /api/v2/organizations/:id/admins | memberV2Handler.ListAdmins | Clerk/APIKey+Org | admin一覧 |
| GET | /api/v2/members/:id/delete-preview | memberV2Handler.DeletePreview | Clerk/APIKey+Org+Admin | メンバー削除プレビュー |
| DELETE | /api/v2/members/:id | memberV2Handler.Delete | Clerk/APIKey+Org+Admin | メンバー削除 |
| PUT | /api/v2/members/:id/admin | memberV2Handler.AssignAdmin | Clerk/APIKey+Org+Admin | admin付与 |
| DELETE | /api/v2/members/:id/admin | memberV2Handler.RevokeAdmin | Clerk/APIKey+Org+Admin | admin剥奪 |
| GET | /api/v2/members | memberV2Handler.List | Clerk/APIKey+Sub | メンバー一覧（v2 members-2ページ用） |
| GET | /api/v2/members/children | memberV2Handler.Children | Clerk/APIKey+Sub | 子メンバー取得 |
| GET | /api/v2/members/:id/objectives | memberV2Handler.Objectives | Clerk/APIKey+Sub | メンバーの目標一覧 |
| PUT | /api/v2/members/:id/pin | memberV2Handler.Pin | Clerk/APIKey+Sub | メンバーピン留め |
| PUT | /api/v2/members/:id/avatar | memberV2Handler.UploadAvatar | Clerk/APIKey+Sub | アバターアップロード |
| PATCH | /api/v2/members/:id/source-organization | memberV2Handler.SetSourceOrganization | Clerk/APIKey+Sub | ソース組織設定 |
| PUT | /api/v2/members/:id | memberV2Handler.Update | Clerk/APIKey+Sub | メンバー更新 |
| GET | /api/v2/members/:id | memberV2Handler.Get | Clerk/APIKey+Sub | メンバー詳細 |
| GET | /api/v2/organizations/:id/member-tags | memberTagHandler.List | Clerk/APIKey+Sub | メンバータグ一覧 |
| POST | /api/v2/organizations/:id/member-tags | memberTagHandler.Create | Clerk/APIKey+Sub+ResolveUser | メンバータグ作成 |
| POST | /api/v2/organizations/:id/members/:memberId/tags | memberTagHandler.Assign | Clerk/APIKey+Sub+ResolveUser | メンバータグ付与 |
| DELETE | /api/v2/organizations/:id/member-tags/:tagId | memberTagHandler.Delete | Clerk/APIKey+Sub+ResolveUser | メンバータグ削除 |
| GET | /api/v2/members/:id/tags | memberTagHandler.ListByMember | Clerk/APIKey+Sub | メンバーのタグ一覧 |
| DELETE | /api/v2/members/:id/tags/:tagId | memberTagHandler.Unassign | Clerk/APIKey+Sub+ResolveUser | タグ解除 |
| POST | /api/v1/team/organization_invitations | organizationInvitationHandler.Create | Clerk/APIKey | 組織招待作成（v1レガシー） |
| POST | /api/v1/team/organization_invitations/accept | organizationInvitationHandler.Accept | Clerk/APIKey | 招待承諾（v1） |
| POST | /api/v1/team/organization_invitations/check_plan_upgrade | organizationInvitationHandler.CheckPlanUpgrade | Clerk/APIKey | プランアップグレード要否確認 |
| GET | /api/v2/invitations/:token | invitationV2Handler.PreviewInvitation | 不要（レート制限のみ） | 招待プレビュー |
| POST | /api/v2/invitations/accept | invitationV2Handler.AcceptInvitation | Clerk | 招待承諾（v2） |
| POST | /api/v2/invitations/:token/accept | invitationV2Handler.AcceptToken | Clerk | トークン指定招待承諾 |
| GET | /api/v2/invitations/pending | invitationV2Handler.ListPending | Clerk | 保留中招待一覧 |
| POST | /api/v2/invitations/pending/:invId/access | invitationV2Handler.CreateAccessToken | Clerk | アクセストークン作成 |
| POST | /api/v2/invitations/decline | invitationV2Handler.DeclineInvitation | Clerk | 招待辞退 |
| POST | /api/v2/invite-links/:code/join | invitationV2Handler.JoinViaLink | Clerk | 招待リンク経由参加 |
| GET | /api/v2/organizations/:id/invited-members | invitationV2Handler.ListInvitedMembers | Clerk/APIKey+Org | 招待中メンバー一覧 |
| GET | /api/v2/organizations/:id/invitation-overview | invitationV2Handler.GetInvitationOverview | Clerk/APIKey+Org | 招待概要 |
| POST | /api/v2/organizations/:id/invitations | invitationV2Handler.CreateInvitations | Clerk/APIKey+Org+Admin | 招待作成（v2） |
| POST | /api/v2/organizations/:id/invitations/:invId/resend | invitationV2Handler.ResendInvitation | Clerk/APIKey+Org+Admin | 招待再送 |
| DELETE | /api/v2/organizations/:id/invitations/:invId | invitationV2Handler.RevokeInvitation | Clerk/APIKey+Org+Admin | 招待取消 |
| POST | /api/v2/organizations/:id/invite-links | invitationV2Handler.CreateInviteLink | Clerk/APIKey+Org+Admin | 招待リンク作成 |
| GET | /api/v2/organizations/:id/invite-links | invitationV2Handler.ListInviteLinks | Clerk/APIKey+Org+Admin | 招待リンク一覧 |
| DELETE | /api/v2/organizations/:id/invite-links/:linkId | invitationV2Handler.DeactivateInviteLink | Clerk/APIKey+Org+Admin | 招待リンク無効化 |

## 9. ゴール/目標 (Objective/Goal) — v1 + v2

v1（`/api/v1/team/objectives`, Clerk/APIKey+Sub）、v2（`/api/v2/objectives`, Clerk/APIKey+Sub）が並行稼働。v2が本流、v1はレガシー互換。

| Method | Path | Handler | 用途 |
|---|---|---|---|
| GET | /api/v1/team/objectives/search | objectiveHandler.Search | 目標検索（v1） |
| GET | /api/v1/team/objectives/:id | objectiveHandler.FindByID | 目標詳細（v1） |
| GET | /api/v1/team/objectives/:id/children | objectiveHandler.GetChildren | 子目標一覧（v1） |
| POST | /api/v1/team/objectives | objectiveHandler.Create | 目標作成（v1） |
| PATCH | /api/v1/team/objectives/:id | objectiveHandler.Update | 目標更新（v1） |
| PATCH | /api/v1/team/objectives/:id/change_parent | objectiveHandler.ChangeParent | 親変更（v1） |
| GET | /api/v1/team/objectives/:id/ancestors | objectiveHandler.GetAncestors | 祖先取得（v1） |
| GET | /api/v1/team/objectives/:id/flat_descendants | objectiveHandler.GetFlatDescendants | 子孫フラット取得（v1） |
| POST | /api/v1/team/objectives/:id/share | objectiveHandler.CreateShareLink | 公開共有リンク作成（v1） |
| DELETE | /api/v1/team/objectives/:id/share | objectiveHandler.RevokeShareLink | 共有リンク失効（v1） |
| GET | /api/v1/team/objectives/:id/aliases | objectiveHandler.ListAliases | エイリアス一覧 |
| POST | /api/v1/team/objectives/:id/aliases | objectiveHandler.CreateAlias | エイリアス作成 |
| PATCH | /api/v1/team/objectives/:id/aliases/reorder | objectiveHandler.ReorderAliases | エイリアス並び替え |
| DELETE | /api/v1/team/objectives/:id/aliases/:aliasId | objectiveHandler.DeleteAlias | エイリアス削除 |
| GET | /api/v1/team/objectives/:id/recurring-goals | objectiveRecurringGoalHandler.GetRecurringGoals | 繰り返しゴール一覧（v1） |
| POST | /api/v2/objectives | objectiveV2Handler.Create | 目標作成（v2） |
| GET | /api/v2/objectives/:id | objectiveV2Handler.FindByID | 目標詳細（v2） |
| PATCH | /api/v2/objectives/:id | objectiveV2Handler.Update | 目標更新（v2） |
| POST | /api/v2/objectives/:id/text-patch | objectiveV2Handler.PatchText | テキストパッチ適用 |
| DELETE | /api/v2/objectives/delete | objectiveV2Handler.Delete | 目標削除 |
| DELETE | /api/v2/objectives/bulk-delete | objectiveV2Handler.BulkDelete | 一括削除 |
| POST | /api/v2/objectives/restore | objectiveV2Handler.Restore | 復元 |
| POST | /api/v2/objectives/archive | objectiveV2Handler.Archive | アーカイブ |
| POST | /api/v2/objectives/unarchive | objectiveV2Handler.Unarchive | アーカイブ解除 |
| GET | /api/v2/objectives/:id/ancestors | objectiveV2Handler.GetAncestors | 祖先取得（v2） |
| GET | /api/v2/objectives/:id/children | objectiveV2Handler.GetChildren | 子取得（v2, `?type=deleted`対応） |
| GET | /api/v2/objectives/:id/descendants | objectiveV2Handler.GetDescendants | 子孫取得 |
| GET | /api/v2/objectives/:id/deliverable-descendants | objectiveV2Handler.GetDeliverableDescendants | 成果物子孫取得 |
| GET | /api/v2/objectives/:id/subtree | objectiveV2Handler.GetSubtree | サブツリー取得 |
| GET | /api/v2/objectives/:id/similar | objectiveDuplicateHandler.FindSimilar | 重複候補検索（find-similar） |
| POST | /api/v2/objectives/:id/parent | objectiveV2Handler.ChangeParent | 親変更（v2） |
| POST | /api/v2/objectives/:id/insert-root | objectiveV2Handler.InsertRoot | ルート挿入 |
| POST | /api/v2/objectives/:id/duplicate | objectiveV2Handler.Duplicate | 複製 |
| GET | /api/v2/objectives/:id/ai-schedule | objectiveV2Handler.GetAISchedule | AIスケジュール取得（ゴール単位・Addy日次アドバイス） |
| PUT | /api/v2/objectives/:id/ai-schedule | objectiveV2Handler.UpsertAISchedule | AIスケジュール更新 |
| GET | /api/v2/organizations/:id/objectives/editable-picker-tree | objectiveV2Handler.GetEditablePickerTree | 親ピッカー初期ツリー取得（Clerk/APIKey+Sub、treePreWarm無し） |
| GET | /api/v2/organizations/:id/objectives/manager-inbox | objectiveMovementV2Handler.ListManagerInbox | マネージャー受信箱一覧 |
| GET | /api/v2/organizations/:id/objectives/:goalId/movement-summary | objectiveMovementV2Handler.GetMovementSummary | ゴール動態サマリ取得 |
| POST | /api/v2/organizations/:id/objectives/:goalId/manager-events | objectiveMovementV2Handler.RecordManagerEvent | マネージャーイベント記録 |
| GET | /api/v2/objectives/:id/kpis | objectiveKPIHandler.List | KPI一覧 |
| POST | /api/v2/objectives/:id/kpis | objectiveKPIHandler.Create | KPI作成 |
| PATCH | /api/v2/objective-kpis/:id | objectiveKPIHandler.Update | KPI更新 |
| DELETE | /api/v2/objective-kpis/:id | objectiveKPIHandler.Delete | KPI削除 |
| GET | /api/v2/objective-kpis/:id/records | objectiveKPIHandler.ListRecords | KPI記録一覧 |
| GET | /api/v2/objectives/:id/suggested-assignees | suggestedAssigneeHandler.List | アサイン候補提案 |
| GET | /api/v2/objectives/:id/subtree/recurring-goals | objectiveRecurringV2Handler.GetSubtreeRecurringGoals | サブツリー内繰り返しゴール取得 |
| GET | /api/v2/objectives/:id/recurring | recurringGoalV2Handler.Get | 繰り返しゴール取得 |
| POST | /api/v2/objectives/:id/recurring | recurringGoalV2Handler.Create | 繰り返しゴール作成 |
| PUT | /api/v2/objectives/:id/recurring | recurringGoalV2Handler.Update | 繰り返しゴール更新 |
| DELETE | /api/v2/objectives/:id/recurring | recurringGoalV2Handler.Delete | 繰り返しゴール削除 |
| GET | /api/v2/organizations/:id/recurring-goals | recurringGoalV2Handler.List | 繰り返しゴール一覧（組織） |
| POST | /api/v2/objective/create | goalExecutionHandler.CreateObjective | ゴール作成（実行タブ簡易作成） |
| POST | /api/v2/validate/objective/create | goalExecutionValidateHandler.CreateObjective | 同上・検証用並走エンドポイント |
| GET | /api/v2/objectives/:id/slack-bindings | objectiveSlackBindingHandler.List | Slackチャンネル紐付け一覧（v1/v2両方に同機能あり） |
| POST | /api/v2/objectives/:id/slack-bindings | objectiveSlackBindingHandler.Create | Slackチャンネル紐付け作成 |
| DELETE | /api/v2/objectives/:id/slack-bindings/:bindingId | objectiveSlackBindingHandler.Delete | Slackチャンネル紐付け削除 |
| GET | /api/v2/objectives/:id/ai-followup-schedule | objectiveAIFollowupScheduleHandler.Get | AIフォローアップスケジュール取得 |
| PUT | /api/v2/objectives/:id/ai-followup-schedule | objectiveAIFollowupScheduleHandler.Put | AIフォローアップスケジュール設定 |
| POST | /api/v1・v2/objectives/:id/assign-ai-agent | aiAgentAssignmentHandler.AssignAIAgent | AIエージェントアサイン（v1/v2両方に同機能あり） |

### AIバックグラウンドタスク（ゴール/組織単位・v1とv2で同一機能セット重複登録）

| Method | Path (v1例) / (v2例) | Handler | 用途 |
|---|---|---|---|
| POST | .../objectives/:id/ai-health-check | aiBackgroundTaskHandler.GoalHealthCheck | AI: ゴールヘルスチェック |
| POST | .../objectives/:id/ai-goal-tree | aiBackgroundTaskHandler.GoalTreeBuild | AI: ゴールツリー構築 |
| POST | .../objectives/:id/ai-completed-review | aiBackgroundTaskHandler.CompletedReview | AI: 完了レビュー |
| POST | .../objectives/:id/ai-bulk-decompose | aiBackgroundTaskHandler.BulkDecompose | AI: 一括分解 |
| POST | .../objectives/:id/ai-create-plan | aiBackgroundTaskHandler.ActionPlan | AI: アクションプラン作成 |
| POST | .../objectives/:id/ai-code-research | aiBackgroundTaskHandler.CodeResearch | AI: コード調査 |
| POST | .../objectives/:id/ai-comment-summary | aiBackgroundTaskHandler.CommentSummary | AI: コメント要約 |
| POST | .../objectives/:id/ai-web-research | aiBackgroundTaskHandler.WebResearch | AI: Web調査 |
| POST | .../objectives/:id/ai-discussion-to-deliverable | aiBackgroundTaskHandler.DiscussionToDeliverable | AI: 議論を成果物化 |
| POST | .../objectives/:id/ai-schedule-check | aiBackgroundTaskHandler.ScheduledPrompt | AI: スケジュールチェック |
| POST | .../objectives/:id/ai-recursive-assign | aiBackgroundTaskHandler.RecursiveAIAssign | AI: 再帰的AIアサイン |
| POST | .../objectives/:id/decompose | goalDecomposeHandler.Decompose | AI目標分解（SSEストリーミング） |
| POST | .../organizations/:id/ai-overdue-goals | aiBackgroundTaskHandler.OverdueGoals | AI: 期限超過ゴール分析 |
| POST | .../organizations/:id/ai-duplicate-detection | aiBackgroundTaskHandler.DuplicateDetection | AI: 重複検出 |
| POST | .../organizations/:id/ai-progress-report | aiBackgroundTaskHandler.ProgressReport | AI: 進捗レポート |
| POST | .../organizations/:id/ai-kpi-review | aiBackgroundTaskHandler.KPIReview | AI: KPIレビュー |
| POST | .../organizations/:id/ai-activity-analysis | aiBackgroundTaskHandler.ActivityAnalysis | AI: 活動分析 |
| POST | .../organizations/:id/ai-workload-analysis | aiBackgroundTaskHandler.WorkloadAnalysis | AI: 作業負荷分析 |
| POST | .../organizations/:id/ai-pr-review | aiBackgroundTaskHandler.PRBulkReview | AI: PR一括レビュー |
| POST | .../organizations/:id/ai-issue-to-goal | aiBackgroundTaskHandler.IssueToGoal | AI: IssueをGoal化 |
| POST | .../organizations/:id/ai-organize-knowledge | aiBackgroundTaskHandler.KnowledgeOrganize | AI: ナレッジ整理 |
| POST | .../organizations/:id/ai-bulk-reminders | aiBackgroundTaskHandler.BulkReminders | AI: 一括リマインド |
| POST | .../organizations/:id/ai-auto-assign | aiBackgroundTaskHandler.AutoAssignMember | AI: 自動アサイン |

（上記23種は `/api/v1/team/objectives`系・`/api/v1/team/organizations`系と`/api/v2/objectives`系・`/api/v2/organizations`系の両方に同一メソッドで重複登録。認証はいずれもClerk/APIKey+Sub）

### 割り当て (Assignment)

| Method | Path | Handler | 認証 | 用途 |
|---|---|---|---|---|
| GET | /api/v2/objectives/:id/assignments | assignmentV2Handler.List | Clerk/APIKey+Sub | アサイン一覧 |
| POST | /api/v2/objectives/:id/assignments | assignmentV2Handler.Create | Clerk/APIKey+Sub | アサイン作成 |
| GET | /api/v2/objectives/:id/assignments/:assignmentId | assignmentV2Handler.Get | Clerk/APIKey+Sub | アサイン詳細 |
| PATCH | /api/v2/objectives/:id/assignments/:assignmentId | assignmentV2Handler.Update | Clerk/APIKey+Sub | アサイン更新 |
| DELETE | /api/v2/objectives/:id/assignments/:assignmentId | assignmentV2Handler.Delete | Clerk/APIKey+Sub | アサイン削除 |
| PUT | /api/v2/objectives/:id/transfer-ownership | assignmentV2Handler.TransferOwnership | Clerk/APIKey+Sub | オーナー移譲 |

### Google Sheets紐付け（`/objectives/:id/sheet-bindings`, v1/v2両方に同機能あり）

| Method | Path (末尾) | Handler | 用途 |
|---|---|---|---|
| GET | /sheet-bindings | objectiveSheetBindingHandler.List | Sheets紐付け一覧 |
| POST | /sheet-bindings | objectiveSheetBindingHandler.Create | Sheets紐付け作成（Google OAuth方式） |
| DELETE | /sheet-bindings/:bindingId | objectiveSheetBindingHandler.Delete | Sheets紐付け削除 |
| GET | /sheet-bindings/service-account-info | objectiveSheetBindingHandler.ServiceAccountInfo | SA共有情報取得（OAuth不要） |
| POST | /sheet-bindings/service-account | objectiveSheetBindingHandler.CreateForServiceAccount | SA方式で紐付け作成 |
| POST | /sheet-bindings/:bindingId/trigger-report | objectiveSheetBindingHandler.TriggerReport | シートレポート即時トリガー（同期5〜15秒） |
| PATCH | /sheet-bindings/:bindingId/schedule | objectiveSheetBindingHandler.UpdateSchedule | シートレポートスケジュール更新 |

## 10. 成果物 (Deliverable)

| Method | Path | Handler | 認証 | 用途 |
|---|---|---|---|---|
| GET | /api/v1/team/objectives/:id/deliverables | deliverableHandler.List | Clerk/APIKey+Sub | 成果物一覧（v1） |
| POST | /api/v1/team/objectives/:id/deliverables | deliverableHandler.Create | Clerk/APIKey+Sub | 成果物作成（v1） |
| GET | /api/v1/team/objectives/:id/deliverables/:deliverableId | deliverableHandler.Show | Clerk/APIKey+Sub | 成果物詳細（v1） |
| POST | /api/v1/team/objectives/:id/deliverables/upload-complete/:deliverableId | deliverableHandler.CompleteUpload | Clerk/APIKey+Sub | アップロード完了通知 |
| PATCH | /api/v1/team/objectives/:id/deliverables/:deliverableId | deliverableHandler.Update | Clerk/APIKey+Sub | 成果物更新 |
| DELETE | /api/v1/team/objectives/:id/deliverables/:deliverableId | deliverableHandler.Delete | Clerk/APIKey+Sub | 成果物削除 |
| PATCH | /api/v1/team/objectives/:id/deliverables/:deliverableId/rename | deliverableHandler.Rename | Clerk/APIKey+Sub | リネーム |
| PATCH | /api/v1/team/objectives/:id/deliverables/:deliverableId/move | deliverableHandler.Move | Clerk/APIKey+Sub | 移動 |
| POST | /api/v1/team/objectives/:id/deliverables/batch_move | deliverableHandler.BatchMove | Clerk/APIKey+Sub | 一括移動 |
| POST | /api/v1/team/objectives/:id/deliverables/batch_delete | deliverableHandler.BatchDelete | Clerk/APIKey+Sub | 一括削除 |
| GET | /api/v1/team/deliverables/:deliverableId | deliverableHandler.Show | Clerk/APIKey+Sub | 成果物詳細（フラットルート、通知からの直接アクセス用） |

## 11. コメント (Comment, v1 DEPRECATED) / Goal Issue（v2チャット型ゴールコメント） / 組織チャット (Org Chat)

`comments`系はDEPRECATED(chat-v1)。goal-issues/goal-sections/chatが後継のv2チャット基盤。

| Method | Path | Handler | 認証 | 用途 | 備考 |
|---|---|---|---|---|---|
| GET | /api/v1/team/comments | commentHandler.List | Clerk/APIKey+Sub | コメント一覧 | DEPRECATED(chat-v1) |
| GET | /api/v1/team/comments/:id/context | commentHandler.FindContext | Clerk/APIKey+Sub | コメント文脈取得 | 同上 |
| GET | /api/v1/team/comments/:id | commentHandler.FindByID | Clerk/APIKey+Sub | コメント詳細 | 同上 |
| POST | /api/v1/team/comments | commentHandler.Create | Clerk/APIKey+Sub | コメント作成 | 同上 |
| PUT | /api/v1/team/comments/:id | commentHandler.Update | Clerk/APIKey+Sub | コメント更新 | 同上 |
| DELETE | /api/v1/team/comments/:id | commentHandler.Delete | Clerk/APIKey+Sub | コメント削除 | 同上 |
| DELETE | /api/v1/team/comments/:id/attachments/:attachmentId | commentHandler.DeleteAttachment | Clerk/APIKey+Sub | コメント添付削除 | 同上 |
| PATCH | /api/v1/team/comments/:id/resolve | commentHandler.Resolve | Clerk/APIKey+Sub | コメント解決 | 同上 |
| PATCH | /api/v1/team/comments/:id/unresolve | commentHandler.Unresolve | Clerk/APIKey+Sub | コメント未解決化 | 同上 |
| POST | /api/v1/team/comments/:id/reactions | commentHandler.AddReaction | Clerk/APIKey+Sub | リアクション追加 | 同上 |
| GET | /api/v1/team/comments/:id/reactions/:emoji/users | commentHandler.GetReactionUsers | Clerk/APIKey+Sub | リアクションユーザー一覧 | 同上 |
| GET | /api/v2/objectives/:id/comments | commentHandler.ListByObjective | Clerk/APIKey+Sub | 目標別コメント一覧 | DEPRECATED(chat-v1) |
| GET | /api/v2/objectives/:id/issues | goalIssueHandler.ListObjectiveIssues | Clerk/APIKey+Sub | 目標のissue一覧 | chat-v2レート制限(polling) |
| POST | /api/v2/objectives/:id/issues | goalIssueHandler.CreateIssue | Clerk/APIKey+Sub | issue作成 | chat-v2レート制限(write) |
| PATCH | /api/v2/objectives/:id/issues/:issueId | goalIssueHandler.EditIssue | Clerk/APIKey+Sub | issue編集 | |
| PUT | /api/v2/objectives/:id/issues/:issueId/read | goalIssueHandler.MarkIssueRead | Clerk/APIKey+Sub | issue既読化 | |
| GET | /api/v2/objectives/:id/issues/:issueId/messages | goalIssueHandler.ListIssueMessages | Clerk/APIKey+Sub | issueメッセージ一覧 | |
| POST | /api/v2/objectives/:id/issues/:issueId/messages | goalIssueHandler.PostIssueMessage | Clerk/APIKey+Sub | issueメッセージ投稿 | |
| PATCH | /api/v2/objectives/:id/issues/:issueId/messages/:messageId | goalIssueHandler.EditIssueMessage | Clerk/APIKey+Sub | issueメッセージ編集 | |
| POST | /api/v2/objectives/:id/issues/:issueId/messages/:messageId/reactions | goalIssueHandler.AddReaction | Clerk/APIKey+Sub | リアクション追加 | |
| DELETE | /api/v2/objectives/:id/issues/:issueId/messages/:messageId/reactions/:emoji | goalIssueHandler.RemoveReaction | Clerk/APIKey+Sub | リアクション削除 | |
| GET | /api/v2/objectives/:id/issues/:issueId/messages/:messageId/reactions/:emoji/users | goalIssueHandler.ListReactionUsers | Clerk/APIKey+Sub | リアクションユーザー一覧 | |
| GET | /api/v2/goal-issues | goalIssueHandler.ListIssues | Clerk/APIKey+Sub | 全issue一覧 | |
| GET | /api/v2/goal-issues/search | goalIssueHandler.SearchMessages | Clerk/APIKey+Sub | issueメッセージ検索 | |
| POST | /api/v2/goal-issues/messages/preview | goalIssueHandler.PreviewMessages | Clerk/APIKey+Sub | 引用リンクのinlineプレビュー（複数id一括取得） | |
| PATCH | /api/v2/goal-issues/:issueId/resolution | goalIssueHandler.SetResolution | Clerk/APIKey+Sub | issue解決状態設定 | |
| GET | /api/v2/goal-sections | goalIssueHandler.ListGoalSections | Clerk/APIKey+Sub | ゴールセクション一覧 | |
| GET | /api/v2/goal-sections/pinned | goalIssueHandler.ListPinnedGoalSections | Clerk/APIKey+Sub | ピン留めセクション一覧 | |
| GET | /api/v2/goal-sections/unread-mention-count | goalIssueHandler.CountUnreadMentions | Clerk/APIKey+Sub | 未読メンション数 | |
| GET | /api/v2/goal-sections/unread-count | goalIssueHandler.CountUnreadComments | Clerk/APIKey+Sub | 未読コメント数 | |
| PATCH | /api/v2/goal-sections/:objectiveId/pinned | goalIssueHandler.SetGoalPinned | Clerk/APIKey+Sub | ピン留め設定 | |
| GET | /api/v2/chat/search | orgChatHandler.SearchMessages | Clerk/APIKey+Sub | チャットメッセージ検索 | |
| GET | /api/v2/chat/rooms | orgChatHandler.ListRooms | Clerk/APIKey+Sub | ルーム一覧 | |
| GET | /api/v2/chat/rooms/public | orgChatHandler.ListPublicGroups | Clerk/APIKey+Sub | 公開グループ一覧 | |
| GET | /api/v2/chat/rooms/unread-count | orgChatHandler.CountGroupUnread | Clerk/APIKey+Sub | 未読数 | |
| GET | /api/v2/chat/rooms/:roomId | orgChatHandler.GetRoom | Clerk/APIKey+Sub | ルーム詳細 | |
| PATCH | /api/v2/chat/rooms/:roomId | orgChatHandler.RenameGroup | Clerk/APIKey+Sub | グループ名変更 | |
| DELETE | /api/v2/chat/rooms/:roomId | orgChatHandler.DeleteGroup | Clerk/APIKey+Sub | グループ削除 | |
| GET | /api/v2/chat/rooms/:roomId/members | orgChatHandler.ListRoomMembers | Clerk/APIKey+Sub | ルームメンバー一覧 | |
| DELETE | /api/v2/chat/rooms/:roomId/members/self | orgChatHandler.LeaveRoom | Clerk/APIKey+Sub | ルーム退出 | |
| DELETE | /api/v2/chat/rooms/:roomId/members/:memberId | orgChatHandler.RemoveMember | Clerk/APIKey+Sub | メンバー削除 | |
| PUT | /api/v2/chat/rooms/:roomId/icon | orgChatHandler.UploadGroupIcon | Clerk/APIKey+Sub | グループアイコン設定 | |
| DELETE | /api/v2/chat/rooms/:roomId/icon | orgChatHandler.DeleteGroupIcon | Clerk/APIKey+Sub | グループアイコン削除 | |
| POST | /api/v2/chat/rooms/:roomId/join | orgChatHandler.JoinPublicGroup | Clerk/APIKey+Sub | 公開グループ参加 | |
| POST | /api/v2/chat/rooms/:roomId/invitations | orgChatHandler.InviteMembers | Clerk/APIKey+Sub | ルーム招待 | |
| GET | /api/v2/chat/invitations/pending | orgChatHandler.ListPendingInvitations | Clerk/APIKey+Sub | 保留中招待一覧（チャット） | |
| POST | /api/v2/chat/invitations/:invitationId/accept | orgChatHandler.AcceptInvitation | Clerk/APIKey+Sub | チャット招待承諾 | |
| POST | /api/v2/chat/invitations/:invitationId/decline | orgChatHandler.DeclineInvitation | Clerk/APIKey+Sub | チャット招待辞退 | |
| GET | /api/v2/chat/rooms/:roomId/messages | orgChatHandler.ListMessages | Clerk/APIKey+Sub | メッセージ一覧 | |
| PUT | /api/v2/chat/rooms/:roomId/read | orgChatHandler.MarkRoomAsRead | Clerk/APIKey+Sub | 既読化 | |
| PATCH | /api/v2/chat/rooms/:roomId/hidden | orgChatHandler.HideRoom | Clerk/APIKey+Sub | ルーム非表示化 | |
| POST | /api/v2/chat/rooms/:roomId/messages | orgChatHandler.PostMessage | Clerk/APIKey+Sub | メッセージ投稿 | |
| PATCH | /api/v2/chat/rooms/:roomId/messages/:messageId | orgChatHandler.EditMessage | Clerk/APIKey+Sub | メッセージ編集 | |
| DELETE | /api/v2/chat/rooms/:roomId/messages/:messageId | orgChatHandler.DeleteMessage | Clerk/APIKey+Sub | メッセージ削除 | |
| POST | /api/v2/chat/rooms/:roomId/messages/:messageId/reactions | orgChatHandler.AddReaction | Clerk/APIKey+Sub | リアクション追加 | |
| DELETE | /api/v2/chat/rooms/:roomId/messages/:messageId/reactions/:emoji | orgChatHandler.RemoveReaction | Clerk/APIKey+Sub | リアクション削除 | |
| GET | /api/v2/chat/rooms/:roomId/messages/:messageId/reactions/:emoji/users | orgChatHandler.ListReactionUsers | Clerk/APIKey+Sub | リアクションユーザー一覧 | |
| POST | /api/v2/chat/dms | orgChatHandler.CreateDM | Clerk/APIKey+Sub | DM作成 | |
| POST | /api/v2/chat/groups | orgChatHandler.CreateGroup | Clerk/APIKey+Sub | グループ作成 | |
| GET | /api/v2/chat/ws | chatDelivery.Handler.ServeWS | 接続後in-band JWT | チャットWebSocket接続（リアルタイム配信） | flag OFF時は未登録。IP単位レート制限あり |
| GET | /api/v2/chat/admin/rooms | orgChatAdminHandler.ListRooms | Clerk/APIKey+Sub | 管理者向け会話ルーム一覧（非参加DM/group含む） | `ORGCHAT_ADMIN_EXPORT_ENABLED` フラグ有効時のみ登録。内部監査用エンドポイント |
| GET | /api/v2/chat/admin/rooms/:roomId/export | orgChatAdminHandler.ExportRoom | Clerk/APIKey+Sub | 会話履歴CSVエクスポート | 同上。adminが非参加ルームの本文閲覧可能なため rollout gate 済み |

## 12. AIスレッド・エージェント (Thread / AI Agent / Agent Memory / Token Usage / Plan Subscription / Onboarding)

| Method | Path | Handler | 認証 | 用途 |
|---|---|---|---|---|
| POST | /api/v1/team/ai/threads | threadHandler.Create | Clerk/APIKey | AIスレッド作成 |
| GET | /api/v1/team/ai/threads | threadHandler.List | Clerk/APIKey | スレッド一覧 |
| GET | /api/v1/team/ai/threads/objective-assignment | threadHandler.GetObjectiveAssignmentThread | Clerk/APIKey | 目標アサイン用スレッド取得 |
| GET | /api/v1/team/ai/threads/:id | threadHandler.Get | Clerk/APIKey | スレッド詳細 |
| PATCH | /api/v1/team/ai/threads/:id | threadHandler.Update | Clerk/APIKey | スレッド更新 |
| DELETE | /api/v1/team/ai/threads/:id | threadHandler.Delete | Clerk/APIKey | スレッド削除 |
| GET | /api/v1/team/ai/threads/:id/messages | threadHandler.GetMessages | Clerk/APIKey | メッセージ一覧 |
| POST | /api/v1/team/ai/threads/:id/chat | threadHandler.Chat | Clerk/APIKey | AIチャット送信（レート制限あり） |
| POST | /api/v1/team/ai/threads/:id/cancel | threadHandler.Cancel | Clerk/APIKey | チャット生成キャンセル |
| PUT | /api/v1/team/ai/threads/:threadId/messages/:messageId/edit-and-regenerate | threadHandler.EditAndRegenerate | Clerk/APIKey | メッセージ編集して再生成 |
| GET | /api/v1/team/ai/threads/:id/traces | actionTraceHandler.ListTraces | Clerk/APIKey | AIアクショントレース一覧 |
| POST | /api/v1/team/ai/threads/:id/traces/:traceId/revert | actionTraceHandler.RevertTrace | Clerk/APIKey | アクションのロールバック |
| POST | /api/v1/team/ai/threads/:id/share | threadHandler.CreateShareLink | Clerk/APIKey | スレッド共有リンク作成 |
| DELETE | /api/v1/team/ai/threads/:id/share | threadHandler.RevokeShareLink | Clerk/APIKey | 共有リンク失効 |
| POST | /api/v1/team/ai/threads/:id/question/respond | questionHandler.RespondToQuestion | Clerk/APIKey | AIからの質問への応答 |
| POST | /api/v1/team/ai/threads/:id/tool-confirmation/respond | toolConfirmationHandler.Respond | Clerk/APIKey | ツール実行承認への応答 |
| GET | /api/v1/team/ai/ai-token-usages/thread/:thread_id | aiTokenUsageHandler.GetByThreadID | Clerk/APIKey | スレッド別トークン使用量 |
| GET | /api/v1/team/ai/ai-token-usages/organization-summary | aiTokenUsageHandler.GetOrganizationSummary | Clerk/APIKey | 組織別トークン使用量サマリ |
| GET | /api/v1/team/ai/ai-token-usages/members-summary | aiTokenUsageHandler.GetOrganizationMembersSummary | Clerk/APIKey | メンバー別トークン使用量サマリ |
| GET | /api/v1/team/ai/token-quota | tokenQuotaHandler.GetTokenQuota | Clerk/APIKey | トークンクォータ取得（microUSDベース） |
| POST | /api/v1/team/ai/usage-ingest | aiUsageIngestHandler.IngestUsage | service（X-Addness-App-Key） | AI使用量取込（外部サービスのLLM消費を残高へ反映） |
| GET | /api/v1/team/ai/token-balance | serviceTokenBalanceHandler.GetBalance | service（X-Addness-App-Key） | トークン残高照会（外部サービス実行前チェック） |
| GET | /api/v1/team/ai/plan-subscriptions/options | aiPlanSubscriptionHandler.GetPlanOptions | Clerk/APIKey | AIプランオプション一覧 |
| GET | /api/v1/team/ai/plan-subscriptions/current | aiPlanSubscriptionHandler.GetCurrentSubscription | Clerk/APIKey | 現在のサブスク取得 |
| POST | /api/v1/team/ai/plan-subscriptions/register | aiPlanSubscriptionHandler.RegisterAIPlan | Clerk/APIKey | プラン登録 |
| PATCH | /api/v1/team/ai/plan-subscriptions/cancel | aiPlanSubscriptionHandler.CancelSubscription | Clerk/APIKey | プラン解約 |
| PATCH | /api/v1/team/ai/plan-subscriptions/change | aiPlanSubscriptionHandler.ChangePlan | Clerk/APIKey | プラン変更 |
| PATCH | /api/v1/team/ai/plan-subscriptions/mode | aiPlanSubscriptionHandler.ChangeBillingMode | Clerk/APIKey | 課金モード変更 |
| DELETE | /api/v1/team/ai/plan-subscriptions/scheduled-downgrade | aiPlanSubscriptionHandler.CancelScheduledDowngrade | Clerk/APIKey | 予約済みダウングレード取消 |
| POST | /api/v1/team/ai/plan-subscriptions/end-trial | aiPlanSubscriptionHandler.EndTrial | Clerk/APIKey | トライアル終了 |
| POST | /api/v1/team/ai/plan-subscriptions/polar/checkout | aiPlanSubscriptionHandler.CreatePolarCheckout | Clerk/APIKey | Polarチェックアウト作成 |
| GET | /api/v1/team/ai/plan-subscriptions/polar/current | aiPlanSubscriptionHandler.CurrentPolarSubscription | Clerk/APIKey | Polar現行サブスク取得 |
| PATCH | /api/v1/team/ai/plan-subscriptions/polar/change | aiPlanSubscriptionHandler.ChangePolarPlan | Clerk/APIKey | Polarプラン変更 |
| PATCH | /api/v1/team/ai/plan-subscriptions/polar/mode | aiPlanSubscriptionHandler.ChangePolarBillingMode | Clerk/APIKey | Polar課金モード変更 |
| POST | /api/v1/team/ai/plan-subscriptions/polar/end-trial | aiPlanSubscriptionHandler.EndPolarTrial | Clerk/APIKey | Polarトライアル終了 |
| POST | /api/v1/team/ai/plan-subscriptions/polar/sync-seats | aiPlanSubscriptionHandler.SyncPolarSeats | Clerk/APIKey | Polarシート数同期 |
| GET | /api/v1/team/ai/plan-subscriptions/polar/customer-portal | aiPlanSubscriptionHandler.PolarCustomerPortal | Clerk/APIKey | Polarカスタマーポータル取得 |
| POST | /api/v1/team/ai/onboarding/interview | onboardingInterviewHandler.Interview | Clerk/APIKey | オンボーディング面談（OpenAI構造化出力）。`OPENAI_API_KEY`未設定時は未登録 |
| POST | /api/v1/team/ai/onboarding/upload | onboardingInterviewHandler.UploadDocument | Clerk/APIKey | オンボーディング資料アップロード |
| POST | /api/v1/team/ai/onboarding/complete | onboardingInterviewHandler.Complete | Clerk/APIKey | オンボーディング完了 |
| POST | /api/v1/team/ai/addness/questions | addnessQuestionsHandler.GetQuestions | Clerk/APIKey+Sub | Addness質問取得（AIチャット向け） |
| GET | /api/v1/team/ai-agents/:id/threads | threadHandler.ListByAgent | Clerk/APIKey+Sub | エージェント別スレッド一覧 |
| GET | /api/v2/organizations/:id/ai-agents | aiAgentHandler.List | Clerk/APIKey+Sub | AIエージェント一覧 |
| GET | /api/v2/organizations/:id/ai-agents/assignable | aiAgentHandler.ListAssignable | Clerk/APIKey+Sub | アサイン可能AIエージェント一覧 |
| POST | /api/v2/organizations/:id/ai-agents | aiAgentHandler.Create | Clerk/APIKey+Sub | AIエージェント作成 |
| GET | /api/v2/organizations/:id/ai-agents/role-templates | aiAgentHandler.ListRoleTemplates | Clerk/APIKey+Sub | ロールテンプレート一覧 |
| GET | /api/v2/ai-agents/:id | aiAgentHandler.FindByID | Clerk/APIKey+Sub | AIエージェント詳細 |
| PATCH | /api/v2/ai-agents/:id | aiAgentHandler.Update | Clerk/APIKey+Sub | エージェント更新 |
| DELETE | /api/v2/ai-agents/:id | aiAgentHandler.Delete | Clerk/APIKey+Sub | エージェント削除 |
| POST | /api/v2/ai-agents/:id/avatar | aiAgentHandler.UploadAvatar | Clerk/APIKey+Sub | アバターアップロード |
| GET | /api/v2/ai-agents/:id/memory/episodes | agentMemoryHandler.ListEpisodes | Clerk/APIKey+Sub | エージェント記憶エピソード一覧 |
| GET | /api/v2/ai-agents/:id/skills | agentMemoryHandler.ListSkills | Clerk/APIKey+Sub | エージェント紐付きスキル一覧 |
| POST | /api/v2/ai-agents/:id/skills | agentMemoryHandler.LinkSkill | Clerk/APIKey+Sub | スキル紐付け |
| DELETE | /api/v2/ai-agents/:id/skills/:skillId | agentMemoryHandler.UnlinkSkill | Clerk/APIKey+Sub | スキル紐付け解除 |
| GET | /api/v2/ai-agents/:id/activity-logs | agentMemoryHandler.ListActivityLogs | Clerk/APIKey+Sub | エージェント活動ログ一覧 |
| GET | /api/v2/ai-agents/:id/credentials | agentMemoryHandler.ListAgentCredentials | Clerk/APIKey+Sub | エージェントクレデンシャル一覧 |
| POST | /api/v2/ai-agents/:id/credentials | agentMemoryHandler.SaveAgentCredential | Clerk/APIKey+Sub | クレデンシャル保存 |
| DELETE | /api/v2/ai-agents/:id/credentials/:serviceType | agentMemoryHandler.DeleteAgentCredential | Clerk/APIKey+Sub | クレデンシャル削除 |
| GET | /api/v2/ai-agents/:id/usage | agentUsageHandler.GetAgentUsage | Clerk/APIKey+Sub | エージェント使用量取得 |
| POST | /api/v2/ai-agents/:id/delegate | aiBackgroundTaskHandler.DelegateTask | Clerk/APIKey+Sub | タスク委任 |

## 13. AIエージェントチャット (Goal Chat / Todo Chat / Core Values / Master Plan) / Goal Decompose

いずれもSSEストリーミング。engine(r)をDIし、ツール実行はbackend自身のv1/v2 RESTをin-processで叩く設計。

| Method | Path | Handler | 認証 | 用途 |
|---|---|---|---|---|
| POST | /api/v2/ai-goal-chat/stream | goalChatHandler.Chat | Clerk/APIKey+Sub | ゴールチャット型エージェント（案A no-context, SSE） |
| GET | /api/v2/ai-goal-chat/encouragement | goalChatHandler.Encouragement | Clerk/APIKey+Sub | 励ましメッセージ取得 |
| GET | /api/v2/ai-goal-chat/threads | goalChatHandler.ListThreads | Clerk/APIKey+Sub | スレッド一覧 |
| GET | /api/v2/ai-goal-chat/threads/:threadId/messages | goalChatHandler.Messages | Clerk/APIKey+Sub | メッセージ一覧 |
| POST | /api/v2/ai-todo-chat/stream | todoChatHandler.Chat | Clerk/APIKey+Sub | 今日のToDoモード（壁打ち型エージェント, SSE） |
| GET | /api/v2/ai-todo-chat/threads | todoChatHandler.ListThreads | Clerk/APIKey+Sub | スレッド一覧 |
| GET | /api/v2/ai-todo-chat/threads/:threadId/messages | todoChatHandler.Messages | Clerk/APIKey+Sub | メッセージ一覧 |
| POST | /api/v2/ai-todo-chat/validate/stream | todoChatValidateHandler.Chat | Clerk/APIKey+Sub | 検証用並走エンドポイント（`internal/aitodochatvalidate`独立実装） |
| GET | /api/v2/ai-todo-chat/validate/threads | todoChatValidateHandler.ListThreads | Clerk/APIKey+Sub | 同上 |
| GET | /api/v2/ai-todo-chat/validate/threads/:threadId/messages | todoChatValidateHandler.Messages | Clerk/APIKey+Sub | 同上 |
| POST | /api/v2/ai-core-values/stream | coreValuesChatHandler.Chat | Clerk/APIKey+Sub | コアバリュー診断モード（純対話, SSE） |
| GET | /api/v2/ai-core-values/threads | coreValuesChatHandler.ListThreads | Clerk/APIKey+Sub | スレッド一覧 |
| GET | /api/v2/ai-core-values/threads/:threadId/messages | coreValuesChatHandler.Messages | Clerk/APIKey+Sub | メッセージ一覧 |
| POST | /api/v2/ai-master-plan/stream | masterPlanChatHandler.Chat | Clerk/APIKey+Sub | マスタープラン診断モード（純対話, SSE） |
| GET | /api/v2/ai-master-plan/threads | masterPlanChatHandler.ListThreads | Clerk/APIKey+Sub | スレッド一覧 |
| GET | /api/v2/ai-master-plan/threads/:threadId/messages | masterPlanChatHandler.Messages | Clerk/APIKey+Sub | メッセージ一覧 |

## 14. 通知 (Notification) / 通知設定 / プッシュトークン / メール宛先

| Method | Path | Handler | 認証 | 用途 |
|---|---|---|---|---|
| GET | /api/v1/team/notification_settings | notificationSettingHandler.List | Clerk/APIKey+Sub | 通知設定一覧 |
| POST | /api/v1/team/notification_settings | notificationSettingHandler.Create | Clerk/APIKey+Sub | 通知設定作成 |
| PATCH | /api/v1/team/notification_settings/:id | notificationSettingHandler.Update | Clerk/APIKey+Sub | 通知設定更新 |
| GET | /api/v1/team/email_destinations | emailDestinationHandler.List | Clerk/APIKey+Sub | メール宛先一覧 |
| GET | /api/v2/organizations/:id/notifications | notificationV2Handler.List | Clerk/APIKey+Sub | 通知一覧 |
| POST | /api/v2/organizations/:id/notifications/mark-read | notificationV2Handler.MarkAsRead | Clerk/APIKey+Sub | 通知既読化 |
| POST | /api/v2/organizations/:id/notifications/mark-unread | notificationV2Handler.MarkAsUnread | Clerk/APIKey+Sub | 通知未読化 |
| POST | /api/v2/organizations/:id/notifications/mark-all-read | notificationV2Handler.MarkAllAsRead | Clerk/APIKey+Sub | 全通知既読化 |
| GET | /api/v2/organizations/:id/notifications/count | notificationV2Handler.Count | Clerk/APIKey+Sub | 未読通知数取得 |
| GET | /api/v2/organizations/:id/notifications/counts-by-objective | notificationV2Handler.CountsByObjective | Clerk/APIKey+Sub | 目標別未読通知数 |
| GET | /api/v2/organizations/:id/notifications/stream | notificationStreamHandler.Stream | Clerk/APIKey+Sub（SSEトークン） | 通知SSEストリーム |

## 15. 実行タブ・カレンダー (Goal Execution: 今日のゴール / 今日のToDo / 計画中ToDo / カレンダー / 履歴)

本番エンドポイントに加え、実行タブ検証用の並走ルート `/validate/...`（`internal/goalexecutionvalidate`から独立実装、DBは本番共有）が同一機能セットで並行登録されている。

| Method | Path | Handler | 認証 | 用途 |
|---|---|---|---|---|
| GET | /api/v2/personal/today-list | goalExecutionHandler.GetPersonalToday | 個人スコープ | 人スコープ・組織横断の「今日」実行リスト取得 |
| GET | /api/v2/personal/daily-activity | goalExecutionHandler.GetPersonalDailyActivity | 個人スコープ | 日次アクティビティ集計（芝生・ストリーク用） |
| GET | /api/v2/codex/todays-goals/view | goalExecutionHandler.GetCodexTodaysGoalsView | Clerk/APIKey+Sub | Codex向け今日のゴールビュー取得 |
| POST | /api/v2/codex/todays-goals/apply | goalExecutionHandler.ApplyCodexTodaysGoals | Clerk/APIKey+Sub | Codex提案の適用 |
| GET | /api/v2/organizations/:id/todays-goals/summary | goalExecutionHandler.GetTodaysGoalsSummary | Clerk/APIKey+Sub | 今日のゴールサマリ |
| GET | /api/v2/organizations/:id/todays-goals | goalExecutionHandler.GetTodaysGoals | Clerk/APIKey+Sub | 今日のゴール一覧 |
| GET | /api/v2/organizations/:id/today-todos | goalExecutionHandler.GetTodayTodos | Clerk/APIKey+Sub | 今日のToDo一覧 |
| POST | /api/v2/organizations/:id/today-todos | goalExecutionHandler.AddTodayTodo | Clerk/APIKey+Sub | 今日のToDo追加 |
| PATCH | /api/v2/organizations/:id/today-todos/:todoId | goalExecutionHandler.UpdateTodayTodoCustom | Clerk/APIKey+Sub | 今日のToDo更新（chat由来行） |
| POST | /api/v2/organizations/:id/today-todos/:todoId/activities | goalExecutionHandler.RecordTodayTodoActivity | Clerk/APIKey+Sub | ToDoアクティビティ記録 |
| DELETE | /api/v2/organizations/:id/today-todos/:todoId | goalExecutionHandler.DeleteTodayTodo | Clerk/APIKey+Sub | 今日のToDo削除（chat由来/objective由来両対応） |
| GET | /api/v2/organizations/:id/planned-todos | goalExecutionHandler.GetPlannedTodos | Clerk/APIKey+Sub | 予定ToDoプール取得 |
| GET | /api/v2/organizations/:id/planned-todos/material | goalExecutionHandler.GetPlannedTodoMaterial | Clerk/APIKey+Sub | ToDo決定材料取得（期限到来・繰り返し・未確定バックログ） |
| POST | /api/v2/organizations/:id/planned-todos | goalExecutionHandler.CreatePlannedTodo | Clerk/APIKey+Sub | 予定ToDo作成 |
| PATCH | /api/v2/organizations/:id/planned-todos/:plannedId | goalExecutionHandler.UpdatePlannedTodo | Clerk/APIKey+Sub | 予定ToDo更新 |
| DELETE | /api/v2/organizations/:id/planned-todos/:plannedId | goalExecutionHandler.DeletePlannedTodo | Clerk/APIKey+Sub | 予定ToDo削除 |
| POST | /api/v2/organizations/:id/planned-todos/:plannedId/adopt | goalExecutionHandler.AdoptPlannedTodo | Clerk/APIKey+Sub | 予定ToDoを今日に採用 |
| GET | /api/v2/organizations/:id/calendar-events | goalExecutionHandler.GetCalendarEvents | Clerk/APIKey+Sub | カレンダーイベント取得 |
| POST | /api/v2/organizations/:id/calendar-events/completion | goalExecutionHandler.CompleteCalendarEvent | Clerk/APIKey+Sub | カレンダーイベント完了記録 |
| GET | /api/v2/organizations/:id/goal-calendar | goalExecutionHandler.GetGoalCalendar | Clerk/APIKey+Sub | ゴールカレンダー取得 |
| GET | /api/v2/organizations/:id/goal-history | goalExecutionHandler.GetGoalHistory | Clerk/APIKey+Sub | ゴール履歴取得 |
| GET | /api/v2/organizations/:id/execute-goals/summary | goalExecutionHandler.GetExecutionSummary | Clerk/APIKey+Sub | 実行サマリ取得 |
| GET | /api/v2/organizations/:id/preferences/goal-collapse | goalExecutionHandler.GetGoalPreference | Clerk/APIKey+Sub | ゴール折りたたみ設定取得 |
| PUT | /api/v2/organizations/:id/preferences/goal-collapse | goalExecutionHandler.UpdateGoalPreference | Clerk/APIKey+Sub | ゴール折りたたみ設定更新 |
| POST | /api/v2/execute-goals/generate | goalExecutionHandler.GenerateExecution | Clerk/APIKey+Sub | 実行計画生成 |
| PUT | /api/v2/execute-goals/:id | goalExecutionHandler.UpdateExecution | Clerk/APIKey+Sub | 実行計画更新 |
| GET | /api/v2/execute-goals/history | goalExecutionHandler.GetHistory | Clerk/APIKey+Sub | 実行履歴取得 |
| GET | /api/v2/todays-goals/active-huddles | huddleComponents.Handler.GetActiveHuddles | Clerk/APIKey+Sub | アクティブミーティング一覧（Huddle無効時はダミー応答） |
| GET / POST / PATCH / DELETE | /api/v2/organizations/:id/validate/... （18本、上記と同一パス構造の`/validate`配下） | goalExecutionValidateHandler.* | Clerk/APIKey+Sub | 実行タブ検証用並走エンドポイント。本番は無改変、検証側だけ独自実装 |
| GET / POST | /api/v2/personal/validate/today-list, /daily-activity | goalExecutionValidateHandler.* | 個人スコープ | 検証用並走エンドポイント |
| GET / POST | /api/v2/codex/validate/todays-goals/view, /apply | goalExecutionValidateHandler.* | Clerk/APIKey+Sub | 検証用並走エンドポイント |

## 16. アクティビティログ (Activity Log)

| Method | Path | Handler | 認証 | 用途 |
|---|---|---|---|---|
| GET | /api/v1/team/organizations/:id/activity-logs/by-member | activityLogHandler.GetByMember | Clerk/APIKey+Sub | メンバー別アクティビティ（v1） |
| GET | /api/v1/team/organizations/:id/activity-logs/objectives/:goalId | activityLogHandler.GetByGoal | Clerk/APIKey+Sub | ゴール別アクティビティ（v1） |
| GET | /api/v1/team/organizations/:id/activity-logs/summary | activityLogHandler.GetSummary | Clerk/APIKey+Sub | アクティビティサマリ（v1） |
| GET | /api/v2/organizations/:id/activity-logs/objectives/:goalId/summary | activityLogV2Handler.GetGoalSummary | Clerk/APIKey+Sub | ゴールアクティビティウィジェット用サマリ（v2） |

## 17. ミーティング (Huddle音声通話 / Meeting Bot / Meeting Note・Minutes)

| Method | Path | Handler | 認証 | 用途 | 備考 |
|---|---|---|---|---|---|
| POST | /api/v2/objectives/:id/huddle/join | huddleComponents.Handler.Join | Clerk/APIKey+Sub | ミーティング参加 | Huddle有効時のみ登録。無効時は `/huddle` GETのみダミー応答 |
| POST | /api/v2/objectives/:id/huddle/leave | huddleComponents.Handler.Leave | Clerk/APIKey+Sub | ミーティング退出 | |
| POST | /api/v2/objectives/:id/huddle/switch | huddleComponents.Handler.Switch | Clerk/APIKey+Sub | ミーティング切替 | |
| GET | /api/v2/objectives/:id/huddle | huddleComponents.Handler.GetStatus | Clerk/APIKey+Sub | ミーティング状態取得 | |
| GET | /api/v2/objectives/:id/huddle/active-subtree | huddleComponents.Handler.GetSubtreeActiveHuddles | Clerk/APIKey+Sub | サブツリー内アクティブミーティング取得 | |
| GET | /api/v2/objectives/:id/huddle/sessions/:sessionId | huddleComponents.Handler.GetSessionStatus | Clerk/APIKey+Sub | セッション状態取得 | |
| POST | /api/v2/objectives/:id/huddle/token | huddleComponents.Handler.Token | Clerk/APIKey+Sub | LiveKitトークン発行 | |
| POST | /api/v2/objectives/:id/huddle/recording/start | huddleComponents.Handler.StartRecording | Clerk/APIKey+Sub | 録音開始 | |
| POST | /api/v2/objectives/:id/huddle/recording/stop | huddleComponents.Handler.StopRecording | Clerk/APIKey+Sub | 録音停止 | |
| POST | /api/v2/objectives/:id/huddle/screen-share | huddleComponents.Handler.AcquireScreenShare | Clerk/APIKey+Sub | 画面共有権取得 | |
| DELETE | /api/v2/objectives/:id/huddle/screen-share | huddleComponents.Handler.ReleaseScreenShare | Clerk/APIKey+Sub | 画面共有権解放 | |
| GET | /api/v2/objectives/:id/huddle/inviteable-members | huddleComponents.Handler.ListInviteableMembers | Clerk/APIKey+Sub | 招待可能メンバー一覧 | |
| GET | /api/v2/huddle/active | huddleComponents.Handler.GetActive | Clerk/APIKey+Sub | 参加中ミーティング取得（フローティングバー用） | |
| GET | /api/v2/huddle/transcription-progress | huddleComponents.Handler.ListMinutesProgress | Clerk/APIKey+Sub | 文字起こし進捗一覧 | |
| POST | /api/v2/huddle/heartbeat | huddleComponents.Handler.Heartbeat | Clerk/APIKey+Sub | ハートビート | |
| POST | /api/v2/huddle/sessions/:sessionId/invitations | huddleComponents.Handler.SendInvitations | Clerk/APIKey+Sub | ミーティング招待送信 | |
| GET | /api/v1/team/meeting-bot/jobs | meetingBotComponents.JobHandler.List | Clerk/APIKey | Meeting Bot（Recall.ai）ジョブ一覧 | JobHandler!=nil時のみ登録 |
| GET | /api/v1/team/meeting-bot/jobs/:id | meetingBotComponents.JobHandler.Get | Clerk/APIKey | ジョブ詳細 | 同上 |
| POST | /api/v1/team/meeting-bot/jobs | meetingBotComponents.JobHandler.Create | Clerk/APIKey | ジョブ作成 | CreateEnabled時のみ登録 |
| DELETE | /api/v1/team/meeting-bot/jobs/:id | meetingBotComponents.JobHandler.Delete | Clerk/APIKey | ジョブ削除 | DeleteEnabled時のみ登録（障害時も課金Bot停止できるよう緩いゲート） |
| POST | /api/v2/meeting-notes/transcribe | meetingNoteHandler.Transcribe | Clerk/APIKey+Sub | 音声文字起こし | |
| POST | /api/v2/meeting-notes/summarize | meetingNoteHandler.Summarize | Clerk/APIKey+Sub | 議事録要約 | |
| POST | /api/v2/meeting-notes/post-minutes | meetingNoteHandler.PostMinutes | Clerk/APIKey+Sub | 議事録投稿 | |
| POST | /api/v2/meeting-notes/suggest-goals | meetingNoteHandler.SuggestGoals | Clerk/APIKey+Sub | ゴール提案（議事録から） | |
| POST | /api/v2/meeting-notes/create-goals | meetingNoteHandler.CreateGoals | Clerk/APIKey+Sub | ゴール作成（議事録から） | |
| POST | /api/v2/minutes | meetingNoteHandler.CreateMinute | Clerk/APIKey+Sub | 議事録作成 | |
| GET | /api/v2/minutes | meetingNoteHandler.ListMinutes | Clerk/APIKey+Sub | 議事録一覧 | |
| GET | /api/v2/minutes/:id | meetingNoteHandler.GetMinute | Clerk/APIKey+Sub | 議事録詳細 | |
| PATCH | /api/v2/minutes/:id | meetingNoteHandler.UpdateMinute | Clerk/APIKey+Sub | 議事録更新 | |
| DELETE | /api/v2/minutes/:id | meetingNoteHandler.DeleteMinute | Clerk/APIKey+Sub | 議事録削除 | |

## 18. ストリーク (Streak)

| Method | Path | Handler | 認証 | 用途 |
|---|---|---|---|---|
| GET | /api/v2/organizations/:id/members/:memberId/streak/share | streakShareHandler.GetShareStatus | Clerk/APIKey+Sub（本人のみ） | ストリーク共有状態取得 |
| POST | /api/v2/organizations/:id/members/:memberId/streak/share | streakShareHandler.CreateShareLink | Clerk/APIKey+Sub（本人のみ） | ストリーク共有リンク作成 |
| DELETE | /api/v2/organizations/:id/members/:memberId/streak/share | streakShareHandler.RevokeShareLink | Clerk/APIKey+Sub（本人のみ） | ストリーク共有リンク失効 |
| POST | /api/v2/organizations/:id/members/:memberId/streak/freeze | streakFreezeHandler.Freeze | Clerk/APIKey+Sub（本人のみ） | ストリークフリーズ |
| DELETE | /api/v2/organizations/:id/members/:memberId/streak/freeze | streakFreezeHandler.Unfreeze | Clerk/APIKey+Sub（本人のみ） | フリーズ解除 |
| POST | /api/v2/organizations/:id/members/:memberId/streak/revive | streakReviveHandler.Revive | Clerk/APIKey+Sub（本人のみ） | ストリーク復活（月1回・繰り越し無し） |
| GET | /api/v2/organizations/:id/members/:memberId/streak | memberV2Handler.Streak | Clerk/APIKey+Sub（本人のみ） | ストリーク取得（streakCount + 過去7日窓） |
| GET | /api/v1/public/streaks/:token | publicStreakHandler.GetStreakByShareToken | 不要（共有トークン） | ストリーク公開共有取得 |

## 19. スキル / ツール (Skill / Tool)

| Method | Path | Handler | 認証 | 用途 |
|---|---|---|---|---|
| POST | /api/v1/team/organizations/:id/skills | skillHandler.CreateSkill | Clerk/APIKey | スキル作成 |
| GET | /api/v1/team/organizations/:id/skills | skillHandler.GetSkills | Clerk/APIKey | スキル一覧 |
| GET | /api/v1/team/organizations/:id/general-skills | skillHandler.GetGeneralSkills | Clerk/APIKey | 汎用スキル一覧 |
| GET | /api/v1/team/organizations/:id/skills/search | skillHandler.SearchSkills | Clerk/APIKey | スキル検索 |
| GET | /api/v1/team/organizations/:id/skills/:skillID | skillHandler.GetSkillByID | Clerk/APIKey | スキル詳細 |
| PUT/PATCH | /api/v1/team/organizations/:id/skills/:skillID | skillHandler.UpdateSkill | Clerk/APIKey | スキル更新 |
| DELETE | /api/v1/team/organizations/:id/skills/:skillID | skillHandler.DeleteSkill | Clerk/APIKey | スキル削除 |
| GET | /api/v1/team/organizations/:id/skills/:skillID/performance | skillHandler.GetSkillPerformance | Clerk/APIKey | スキルパフォーマンス取得 |
| POST | /api/v1/team/organizations/:id/skills/:skillID/resources | skillResourceHandler.CreateResource | Clerk/APIKey | スキルリソース作成 |
| GET | /api/v1/team/organizations/:id/skills/:skillID/resources | skillResourceHandler.ListResources | Clerk/APIKey | スキルリソース一覧 |
| GET | /api/v1/team/organizations/:id/skills/:skillID/resources/:resourceID | skillResourceHandler.GetResource | Clerk/APIKey | スキルリソース詳細 |
| PUT | /api/v1/team/organizations/:id/skills/:skillID/resources/:resourceID | skillResourceHandler.UpdateResource | Clerk/APIKey | スキルリソース更新 |
| DELETE | /api/v1/team/organizations/:id/skills/:skillID/resources/:resourceID | skillResourceHandler.DeleteResource | Clerk/APIKey | スキルリソース削除 |
| POST | /api/v1/team/organizations/:id/skills/refinements/:refinementID/accept | skillHandler.AcceptSkillRefinement | Clerk/APIKey | スキル改善提案の承認 |
| POST | /api/v1/team/organizations/:id/skills/refinements/:refinementID/reject | skillHandler.RejectSkillRefinement | Clerk/APIKey | スキル改善提案の却下 |
| POST | /api/v1/team/organizations/:id/tools | toolHandler.CreateTool | Clerk/APIKey | ツール作成 |
| GET | /api/v1/team/organizations/:id/tools | toolHandler.GetTools | Clerk/APIKey | ツール一覧 |
| GET | /api/v1/team/organizations/:id/tools/search | toolHandler.SearchTools | Clerk/APIKey | ツール検索 |
| GET | /api/v1/team/organizations/:id/tools/:toolID | toolHandler.GetToolByID | Clerk/APIKey | ツール詳細 |
| PUT/PATCH | /api/v1/team/organizations/:id/tools/:toolID | toolHandler.UpdateTool | Clerk/APIKey | ツール更新 |
| DELETE | /api/v1/team/organizations/:id/tools/:toolID | toolHandler.DeleteTool | Clerk/APIKey | ツール削除 |
| POST | /api/v1/team/organizations/:id/tools/:toolID/execute | toolHandler.ExecuteTool | Clerk/APIKey | ツール実行 |

## 20. 個人スペース (Personal)

| Method | Path | Handler | 認証 | 用途 |
|---|---|---|---|---|
| GET | /api/v2/personal/now | personalHandler.GetNow | 個人スコープ | 現在時刻取得 |
| GET | /api/v2/personal/today | personalHandler.GetToday | 個人スコープ | 今日のMarkdownドキュメント取得 |
| POST | /api/v2/personal/today/append | personalHandler.AppendToday | 個人スコープ | 今日ドキュメントへの追記 |
| POST | /api/v2/personal/text-patch | personalHandler.PatchText | 個人スコープ | テキストパッチ適用 |
| GET | /api/v2/personal/markdown/analyze | personalHandler.AnalyzeMarkdown | 個人スコープ | Markdown解析 |
| POST | /api/v2/personal/markdown/replace-section | personalHandler.ReplaceMarkdownSection | 個人スコープ | セクション置換 |
| POST | /api/v2/personal/markdown/upsert-section | personalHandler.UpsertMarkdownSection | 個人スコープ | セクションupsert |
| POST | /api/v2/personal/markdown/upsert-list-item | personalHandler.UpsertMarkdownListItem | 個人スコープ | リスト項目upsert |
| POST | /api/v2/personal/markdown/replace-document | personalHandler.ReplaceMarkdownDocument | 個人スコープ | ドキュメント全置換 |
| POST | /api/v2/personal/markdown/append-log-entry | personalHandler.AppendMarkdownLogEntry | 個人スコープ | ログエントリ追記 |
| GET | /api/v2/personal/days/:date | personalHandler.GetDailyEntry | 個人スコープ | 日別エントリ取得 |
| GET | /api/v2/personal/agent-sessions | personalHandler.ListAgentSessions | 個人スコープ | エージェントセッション一覧 |
| POST | /api/v2/personal/agent-sessions | personalHandler.CreateAgentSession | 個人スコープ | エージェントセッション作成 |
| GET | /api/v2/personal/agent-sessions/:id | personalHandler.GetAgentSession | 個人スコープ | セッション詳細 |
| PATCH | /api/v2/personal/agent-sessions/:id | personalHandler.UpdateAgentSession | 個人スコープ | セッション更新 |
| GET | /api/v2/personal/projects | personalHandler.ListProjects | 個人スコープ | プロジェクト一覧 |
| POST | /api/v2/personal/projects | personalHandler.CreateProject | 個人スコープ | プロジェクト作成 |
| GET | /api/v2/personal/projects/:id | personalHandler.GetProject | 個人スコープ | プロジェクト詳細 |
| PATCH | /api/v2/personal/projects/:id | personalHandler.UpdateProject | 個人スコープ | プロジェクト更新 |
| DELETE | /api/v2/personal/reset | personalHandler.ResetPersonal | 個人スコープ | 個人データリセット |
| POST | /api/v1/team/personal-organization/ensure | personalOrganizationHandler.Ensure | Clerk/APIKey | 個人組織（Chat/Perfect Days課金先）の冪等ensure |

## 21. 検索 (Search) / 診断 (Diagnosis) / 紹介 (Referral) / 請求書 (Invoice) / ゴールツリー共有 / インラインメディア

| Method | Path | Handler | 認証 | 用途 |
|---|---|---|---|---|
| GET | /api/v1/team/search | searchHandler.Search | Clerk/APIKey+Sub | 統合検索（目標・コメント。DoS防止レート制限あり） |
| GET | /api/v1/public/diagnosis-results/stats | diagnosisHandler.GetStats | 不要 | 診断結果の匿名集計（タイプ別件数・総数のみ） |
| POST | /api/v2/me/diagnosis-results | diagnosisHandler.SaveResult | Clerk/APIKey（個人スコープ） | 診断結果保存（再診断は履歴追記、利用は最新1件） |
| GET | /api/v2/me/diagnosis-results | diagnosisHandler.GetMyResults | Clerk/APIKey（個人スコープ） | 診断結果一覧取得（マイページ用） |
| GET | /api/v2/me/diagnosis-results/:kind | diagnosisHandler.GetMyResultByKind | Clerk/APIKey（個人スコープ） | 種別別診断結果フル取得（シート表示用） |
| GET | /api/v2/organizations/:id/me/diagnosis-visibility | diagnosisHandler.GetMyVisibility | Clerk専用 | 診断プロフィール公開設定取得 |
| PATCH | /api/v2/organizations/:id/me/diagnosis-visibility | diagnosisHandler.UpdateMyVisibility | Clerk専用 | 診断プロフィール公開設定更新（課金停止中でも変更可） |
| GET | /api/v2/organizations/:id/member-diagnosis-profiles | diagnosisHandler.ListMemberProfiles | Clerk/APIKey+Sub | メンバー診断プロフィール一覧 |
| GET | /api/v2/organizations/:id/members/:memberId/diagnosis-profile | diagnosisHandler.GetMemberProfile | Clerk/APIKey+Sub | メンバー診断プロフィール取得 |
| GET | /api/v1/team/referrals/me | referralHandler.MyReferrals | Clerk/APIKey | 自分の紹介実績取得 |
| POST | /api/v1/team/referrals/conversions | referralHandler.ConvertSignup | Clerk/APIKey | 紹介コンバージョン記録 |
| GET | /api/v1/admin/referrals/logs | referralHandler.AdminLogs | Clerk+Admin | 紹介ログ（管理者） |
| GET | /api/v1/team/invoices | invoiceHandler.ListInvoice | Clerk/APIKey | 請求書一覧 |
| GET | /api/v1/public/objectives/:publicId | publicObjectiveHandler.GetObjectiveByPublicID | 不要 | 目標の公開共有取得 |
| GET | /api/v1/public/goal-trees/:publicId | sharedGoalTreeComponents.Handler.GetPublic | 不要 | ゴールツリー公開共有取得 |
| POST | /api/v2/share-trees | sharedGoalTreeComponents.Handler.Create | Clerk/APIKey+Sub | ゴールツリー共有作成 |
| DELETE | /api/v2/share-trees/:id | sharedGoalTreeComponents.Handler.Revoke | Clerk/APIKey+Sub | 共有失効 |
| GET | /api/v2/share-trees/mine | sharedGoalTreeComponents.Handler.ListMine | Clerk/APIKey+Sub | 自分の共有一覧 |
| POST | /api/v2/share-trees/clones | sharedGoalTreeComponents.Handler.Clone | Clerk/APIKey+Sub | 共有ツリーのクローン |
| GET | /api/v1/public/ai/threads/:token | threadHandler.GetThreadByShareToken | 不要（共有トークン） | AI共有スレッド取得 |
| GET | /api/v2/inline-media/:id | inlineMediaV2Handler.View | Clerk/APIKey+Sub | インラインメディア表示用リダイレクト（S3 presigned GET、毎回発行） |
| POST | /api/v2/organizations/:id/objectives/:goalId/inline-media/upload-url | inlineMediaV2Handler.UploadInit | Clerk/APIKey+Sub | インラインメディアアップロードURL発行 |
| GET | /api/v2/organizations/:id/objectives/:goalId/report-schedule | goalReportV2Handler.GetReportSchedule | Clerk/APIKey+Sub | ゴール活動レポート配信スケジュール取得（per-goal×per-member） |
| PUT | /api/v2/organizations/:id/objectives/:goalId/report-schedule | goalReportV2Handler.UpsertReportSchedule | Clerk/APIKey+Sub | レポートスケジュール更新 |
| DELETE | /api/v2/organizations/:id/objectives/:goalId/report-schedule | goalReportV2Handler.DeleteReportSchedule | Clerk/APIKey+Sub | レポートスケジュール削除 |
| POST | /api/v2/organizations/:id/objectives/:goalId/report-schedule/test | goalReportV2Handler.SendTestReport | Clerk/APIKey+Sub | レポート即時テスト送信 | リリース前削除予定の内部検証用エンドポイント |

## 22. Codexジョブ (v1 / v2)

| Method | Path | Handler | 認証 | 用途 | 備考 |
|---|---|---|---|---|---|
| GET | /api/v1/codex/jobs | codexComponents.Handler.List | Clerk/APIKey | codexジョブ一覧（v1） | |
| GET | /api/v1/codex/jobs/:id | codexComponents.Handler.Get | Clerk/APIKey | ジョブ詳細（v1） | |
| POST | /api/v1/codex/jobs | codexComponents.Handler.Create | Clerk/APIKey | ジョブ作成（v1） | |
| POST | /api/v1/codex/jobs/:id/input | codexComponents.Handler.Input | Clerk/APIKey | 入力送信（v1） | |
| POST | /api/v1/codex/jobs/:id/resume | codexComponents.Handler.Resume | Clerk/APIKey | 再開（v1） | |
| POST | /api/v1/codex/jobs/:id/close | codexComponents.Handler.Close | Clerk/APIKey | クローズ（v1） | |
| POST | /api/v1/codex/jobs/:id/cancel | codexComponents.Handler.Cancel | Clerk/APIKey | キャンセル（v1） | |
| GET | /api/v1/codex/jobs/:id/events | codexComponents.Handler.Stream | Clerk/APIKey+SSEトークン | イベントストリーム（v1, SSE） | |
| GET | /api/v2/codex/jobs | codexComponents.Handler.List | Clerk/APIKey | codexジョブ一覧（v2） | |
| GET | /api/v2/codex/jobs/:id | codexComponents.Handler.Get | Clerk/APIKey | ジョブ詳細（v2） | |
| POST | /api/v2/codex/jobs | codexComponents.Handler.Create | Clerk/APIKey | ジョブ作成（v2） | |
| POST | /api/v2/codex/jobs/:id/input | codexComponents.Handler.Input | Clerk/APIKey | 入力送信（v2） | |
| POST | /api/v2/codex/jobs/:id/resume | codexComponents.Handler.Resume | Clerk/APIKey | 再開（v2） | |
| POST | /api/v2/codex/jobs/:id/close | codexComponents.Handler.Close | Clerk/APIKey | クローズ（v2） | |
| POST | /api/v2/codex/jobs/:id/cancel | codexComponents.Handler.Cancel | Clerk/APIKey | キャンセル（v2） | |
| DELETE | /api/v2/codex/jobs/:id | codexComponents.Handler.Delete | Clerk/APIKey | ジョブ削除（v2のみ） | |
| GET | /api/v2/codex/jobs/:id/events | codexComponents.Handler.Stream | Clerk/APIKey+SSEトークン | イベントストリーム（v2, SSE） | |

## 23. 管理者 (Admin)

| Method | Path | Handler | 認証 | 用途 | 備考 |
|---|---|---|---|---|---|
| GET | /api/v1/admin/organizations/dashboard | adminOrganizationHandler.GetDashboard | Clerk+Admin | 組織ダッシュボード | 内部管理者専用エンドポイント |
| PATCH | /api/v1/admin/organizations/:id/update_subscription_info | adminOrganizationHandler.UpdateSubscriptionInfo | Clerk+Admin | サブスク情報更新 | 同上 |
| GET | /api/v1/admin/ai-plan-subscriptions | adminAIPlanSubscriptionHandler.List | Clerk+Admin | AIプランサブスク一覧 | 同上 |
| GET | /api/v1/admin/ai-token-usages/global-summary | adminAITokenUsageHandler.GetGlobalSummary | Clerk+Admin | 全体トークン使用量サマリ | 同上 |
| GET | /api/v1/admin/referrals/logs | referralHandler.AdminLogs | Clerk+Admin | 紹介ログ | 同上 |

## 24. ALB用エイリアスルート（`/api/ai/v1`, `/api/ai/v2`）

レスポンスタイムが遅いエンドポイントをALBパスマッチングで段階移行するための別名ルート。ハンドラは`/api/v1/team/*`・`/api/v2/*`と共用（重複登録）。

| Method | Path | Handler | 認証 | 用途 | 備考 |
|---|---|---|---|---|---|
| POST | /api/ai/v1/threads/:id/chat | threadHandler.Chat | Clerk | AIチャット送信 | `/api/v1/team/ai/threads/:id/chat`のALBエイリアス。APIKey認証は非対応（Clerkのみ） |
| GET | /api/ai/v1/threads | threadHandler.List | Clerk | スレッド一覧 | 同上 |
| DELETE | /api/ai/v1/threads/:id | threadHandler.Delete | Clerk | スレッド削除 | 同上 |
| PUT | /api/ai/v1/threads/:threadId/messages/:messageId/edit-and-regenerate | threadHandler.EditAndRegenerate | Clerk | 編集して再生成 | 同上 |
| GET | /api/ai/v1/ai-agents/:id/threads | threadHandler.ListByAgent | Clerk+Sub | エージェント別スレッド一覧 | 同上 |
| GET | /api/ai/v2/organizations/:id/ai-agents | aiAgentHandler.List | Clerk+Sub | AIエージェント一覧 | `/api/v2/organizations/:id/ai-agents`のALBエイリアス |
| GET | /api/ai/v2/organizations/:id/ai-agents/assignable | aiAgentHandler.ListAssignable | Clerk+Sub | アサイン可能エージェント一覧 | 同上 |

---

## サマリ：リソースグループ別エンドポイント数

| # | リソースグループ | エンドポイント数（登録行ベース） |
|---|---|---|
| 1 | システム / ヘルスチェック | 2 |
| 2 | 認証・MCP OAuth・APIキー・デスクトップ認証 | 18 |
| 3 | Webhook | 9 |
| 4 | 外部連携（Slack/Discord/GitHub/Google/LINE/Zoom/Codex Integrations） | 47 |
| 5 | MCPプロトコル | 3 |
| 6 | ユーザー / ユーザー設定 | 9 |
| 7 | 組織 (Organization) | 26 |
| 8 | メンバー / メンバータグ / 招待 | 39 |
| 9 | ゴール/目標（v1+v2 CRUD・階層・共有・エイリアス・KPI・AIスケジュール・Sheets紐付け） | 60 |
| 9a | └ AIバックグラウンドタスク（ゴール/組織単位、v1+v2重複） | 46（23機能×v1/v2） |
| 9b | └ 割り当て (Assignment) | 6 |
| 10 | 成果物 (Deliverable) | 11 |
| 11 | コメント(v1 DEPRECATED) / Goal Issue / 組織チャット | 56 |
| 12 | AIスレッド・エージェント（Thread/Agent/TokenUsage/PlanSubscription/Onboarding） | 55 |
| 13 | AIエージェントチャット（Goal Chat/Todo Chat/Core Values/Master Plan） | 16 |
| 14 | 通知 / 通知設定 / メール宛先 | 11 |
| 15 | 実行タブ・カレンダー（今日のゴール/ToDo/計画/履歴、検証用並走含む） | 51（本番27 + validate系24） |
| 16 | アクティビティログ | 4 |
| 17 | ミーティング（Huddle/Meeting Bot/Meeting Note・Minutes） | 30 |
| 18 | ストリーク | 8 |
| 19 | スキル / ツール | 20 |
| 20 | 個人スペース (Personal) | 21 |
| 21 | 検索 / 診断 / 紹介 / 請求書 / ゴールツリー共有 / インラインメディア | 24 |
| 22 | Codexジョブ (v1+v2) | 16 |
| 23 | 管理者 (Admin) | 5 |
| 24 | ALB用エイリアスルート | 7 |

**総エンドポイント数（`registerMCPRoute`のAny(3本)含む、grep実測値）: 659本**
（コード上の `r.GET/POST/PUT/PATCH/DELETE/Any(...)` 直接呼び出しと `postJSON/patchJSON/putJSON/deleteJSON/*NoContent` ヘルパー経由の登録を`presentation/routes/api.go`から機械的に集計。上記サマリの内訳合計とは、v1/v2/validate/ALBの重複カウント方法の違いにより多少のズレがあるが、両者とも「実装上の登録個数」を指しており、実質的なユニークURL数（パスパターンの重複除く）は概ね450〜480程度と推定される）

### 主要な重複パターン（設計上の意図的な二重化）

- **v1 (`/api/v1/team/...`) と v2 (`/api/v2/...`)**: objectives/organizations配下のAIバックグラウンドタスク・目標CRUD・Sheets紐付け・AIエージェントアサイン等が同一機能で並行稼働（v1はレガシー互換）。
- **本番と`/validate`検証ルート**: 実行タブ（goal execution）・今日のToDoモード（ai-todo-chat）が、`internal/goalexecutionvalidate` / `internal/aitodochatvalidate` という別モジュールの独立実装で本番と並走登録されている（本番コード無改変で挙動検証するための内部専用ルート）。
- **`/api/v1/team/*` と `/api/ai/v1`・`/api/v2/*` と `/api/ai/v2`**: ALBパスマッチングによる段階移行用エイリアス（ハンドラ実体は共用）。

---

関連ドキュメント: [`docs/cli-endpoint-coverage.md`](./cli-endpoint-coverage.md)（本表を基にしたCLIギャップ対応表）
