# REST CLI への移行（未リリース）

`addness mcp` と MCP クライアントの通信実装を撤去する。CLI は専用コマンドから REST API を直接呼ぶ。サーバーの MCP カタログを取得したり、未対応操作を MCP に転送したりしない。TUI で起動する外部エージェント自身の MCP 設定は別機能。

## 今回の対応

| 従来の読み取り | REST CLI | 取得範囲 |
|---|---|---|
| `list_member_goals` | `member objectives <MEMBER_ID> --json`（`member goals` も可） | 担当ゴール1ページ。既定100件、未完了、子目標数なし。祖先・子孫の自動取得はしない |
| `read_goal_detail` | `goal detail <GOAL_ID> --json` | ゴール1件の全文・版をRESTのJSONのまま返す。本文は保存形式で、Markdown変換・文字数での分割はしない |
| `get_crm_list` | `crm get <LIST_ID> --json` と必要時だけ `crm rows <LIST_ID> --json` | 設定と顧客行を別々に1回ずつ取得。行のページ送りは `--cursor` |
| `list_crm_lists` | `crm list --json` | 名前・責任者・タグ・フォルダで絞り込んだ1ページ。`--cursor` を保存して続きへ |
| `read_chat_attachment` | `chat attachment --room <ROOM_ID> --message <MESSAGE_ID> --attachment <ATTACHMENT_ID> --output <PATH> --json` | 添付の署名URLをRESTで取得し、ファイル本体を保存。最大64MiB。既存ファイルは上書きしない。画像は返った `path` を表示ツールで開く |

`member objectives` は従来の「全件・完了分・子目標数つき」から変わる。必要な時だけ `--include-completed` / `--include-counts` を付け、返された `total` / `has_more` と取得件数に従って `--offset` を進める。`--created-after` はRFC3339。`--org` で対象組織を指定できる。

MCP の表示用加工、フォルダパスの補完、一覧からの関連情報の展開は移植しない。各コマンドの応答はRESTのJSONを保持する。`goal detail` は現存ゴール用で、削除済み・過去スナップショットへの自動フォールバックはない。これらをMCPと完全互換の出力として扱わない。

既存の `goal`、`form`、`comment`、`deliverable`、`chat`、`challenge` などの専用コマンドは引き続き使える。Analytics/KPI、CRMの書き込み、汎用Drive、ナレッジ、録画一覧、個人ルーティン、エージェント管理など、旧 `mcp call` だけで使えた残りの機能は、この変更では専用コマンド未対応。必要な操作が `--help` に無い場合は未対応と報告する。

## 呼び出しが増え続ける処理への対策

429 の `Retry-After`（秒数またはHTTP日付）を保存し、同じ接続先・APIキーのCLIプロセス間で待機期限を共有する。最低60秒、待機後も制限に当たれば2倍に延ばし、クライアント側の待機は最大1時間。サーバーがより長い待機を指定した時は、その期限を守る。1時間以上制限が無ければ次回の待機は60秒から再開する。

期限内の要求はローカルで非ゼロ終了し、APIへ送らない。429を自動再送しない。状態はOSのキャッシュ領域の `addness/rate-limits`（`ADDNESS_CACHE_DIR` 指定時はその配下の `rate-limits`）に置き、接続先とキーのSHA-256をファイル名に使う。読み書きのロックにより、同時に返った429が長い待機期限を短縮しない。保存先に問題がある時は、送信を続けずエラーを返す。

これは新しいCLIを同じ端末・同じキャッシュ領域で使う時の対策。既に動いている旧版や別端末の処理を停止するものではない。該当する自動処理の持ち主による更新・呼び出し間隔の修正と、本番での呼び出し数の再確認が必要。

## 確認した契約

backend `9a0920b878` の `docs/api/members_v2/openapi.yml`、`docs/api/crm/openapi.yml`、`docs/api/chat/openapi.yml` とゴール詳細ハンドラを参照。ローカルHTTPと実CLIバイナリで通信回数・ページ条件・JSON保持・プロセス間の429抑止を検証する。本番へ書き込みを行うテストはしない。
