# MCP/CLI 対応と API 移行（v0.15.0）

確認基準は backend `1fa0c21607`（2026-10-11）の `cmd/server/router`、各 feature の route/handler、`internal/mcp/handler/catalog.go`。本番の組織 MCP カタログは CLI 自身で取得し、301 ツールを確認した。カタログは権限・入口・サーバー更新で変わるため、301 は固定の上限ではない。

## MCP の機能差分

`addness mcp list --json` は `tools/list` の全ページ、`schema <name>` はその時点のツール定義、`call <name>` は `tools/call` を扱う。CLI 内の手書きツール一覧を経由しないため、Analytics/KPI、CRM、MA、Drive、テンプレート、フォーム、ナレッジ、録画、共有・権限、エージェント機能を含む、選択した入口で公開中の全ツールを呼び出せる。

| CLI | 動作 |
|---|---|
| `mcp list [--query <text>] --json` | 最新カタログを全ページ取得して絞り込む |
| `mcp schema <name> --json` | 入力 JSON Schema、説明、annotations を表示。実行しない |
| `mcp call <name> --args '<JSON>' --json` | JSON オブジェクトを渡して一度実行 |
| `mcp call <name> --args-file <path\|-> --json` | ファイルまたは標準入力から引数を読む |
| `mcp info --json` | 初期化結果、サーバーの指示・機能、交渉済みプロトコルを表示 |

`--scope organization|personal|copilot|openai|admin|support` はサーバーの入口を選ぶ。権限の判定はサーバーが行う。個人用・運営用のキーは `ADDNESS_API_TOKEN` で指定できる。`--org` はその呼び出しにだけ適用し、保存済み組織設定は変更しない。

これは各機能の専用サブコマンドを301個追加する方式ではなく、最新の MCP 入力仕様を CLI から使う入口である。ウィジェットの表示やブラウザ操作は行わず、結果のテキスト・構造化データ・リソース参照を保持する。データのページ送りは各ツールの仕様に従う。

## 既存コマンドの移行

| 既存コマンド | v0.15.0 の扱い・移行先 |
|---|---|
| `comment` | 現行 `/api/v2/comments` とゴール別 comments API に移行。本文・返信先・メンションをそのまま送る。Goal Issue の文字数制限による経路切替と v1 fallback を削除 |
| `comment list-all` | 現行 API は投稿者別。`--author` 必須、絞り込みは resolved/limit/offset。ゴール・返信の絞り込みは `comment list --goal` |
| `deliverable` / `link pr` | 新 Drive のゴールフォルダ・リンク・ファイル・本文更新・改名・ゴミ箱へ移行。Markdown はファイルとして保存。削除・更新前にゴールとの紐づきを検証 |
| `deliverable move/batch-move` / `update --mention` | 旧順序番号・ドキュメントメンション契約を撤去。移動は `mcp schema move_drive_resource` で現行仕様を確認。TUI のフォルダ移動は新 Drive に移行 |
| `search` / `goal search` / `goal share create/revoke` | 現行 v2 API に移行 |
| `codex-job` / `skill` | 旧バックエンドを撤去済みのため削除。現行エージェント操作は `mcp list --query agent --json` で確認。旧クラウドジョブとの一対一の互換性はない |
| `share-tree` / `goal share get-public` | 廃止された公開ツリー API を削除。現行の共有・テンプレート操作は `mcp list --query share --json` |
| `goal duplicate/alias/recurring` | 廃止済み API を削除。型の適用・定期実行はテンプレートや個人ルーティンのツールを確認。旧スケジュールを別契約へ自動変換しない |
| `personal now/today/today-append/day/text-patch/markdown/agent-session/project/reset` | 旧個人ドキュメント API を削除。`personal ensure-organization/today-list/daily-activity` は残す。現行 Perfect Days は個人用キーで `mcp --scope personal list --json` |
| `meeting notes/minutes` | 旧文字起こし・Minutes CRUD API を削除。現行録画・議事録は MCP と Drive |
| `meeting huddle active/transcription-progress` | 410 を返す旧ポーリング API を削除。残る huddle status/active-subtree 等を利用 |
| `meeting bot list/delete` | 廃止された一覧・削除 API を削除。録画停止は `meeting bot stop <id>`。`create` は現行の `--meeting-url` / `--meeting-title` / `--drive-folder` / `--goal` / `--record-video` に移行。旧 `--platform` / `--bot-name` / `--chat-join-message` は削除 |
| `execution codex apply` | 廃止済み一括適用 API を削除。`view` は現行 API に残る |
| `org subscription/ai-agent-member` / `invoice` | 撤去された旧課金・AIメンバー API を削除 |
| `user list/create/rm` / `comment attachment rm` / `invitation legacy-accept/check-plan-upgrade` | 対応する現行ルートがないため削除 |
| `invitation accept/accept-token/pending/decline/link join` / `api-key` / `diagnosis visibility` / `org admin-check` | 現行ルートは Clerk 認証限定。CLI の API キーでは使えないため削除し、Web 画面へ移行 |

旧コマンドの enum、dispatch、API メソッド、DTO、使われなくなった補助コードも削除した。過去の `cli-endpoint-coverage.md` の数値は旧断面の記録であり、現行の対応数には使わない。

## 通信と検証

- [MCP Streamable HTTP](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports) の initialize、initialized、プロトコル・セッションヘッダー、JSON/SSE 応答を扱う。
- MCP 呼び出しと REST 書き込みはタイムアウト後に自動再送しない。完了済みの書き込みを重複させないため、結果を確認してから再実行する。
- 成果物一覧は Drive の全ページを取得する。取得途中で失敗した場合やページが進まない場合は、一覧を全件取得した扱いにしない。
- MCP の `isError: true` は JSON を出力したうえで非ゼロ終了。HTTP/JSON-RPC エラーも成功扱いにしない。
- モック HTTP と実バイナリのテストで、カタログのページ送り、将来のツール名、SSE、セッション、JSON 引数・標準入力・エラー終了、コメント/Drive/検索/共有の API 契約を検証する。
- 本番での確認はカタログ・スキーマ取得のみ。書き込みの検証はモック HTTP を使い、本番データは変更しない。
