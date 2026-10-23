use anyhow::Result;

const SKILLS_PROMPT: &str = r#"# Addness CLI Skills

## Addnessとは

Addnessは「AIと一緒に人生を進める新しいOS」です。
個人やチームのゴール（目標）をツリー構造で管理し、進捗の可視化・振り返り・コメントによるコミュニケーションを通じて目標達成を支援するプラットフォームです。

Addness CLIはターミナルからAddnessのゴール管理機能を操作するためのコマンドラインツールです。
すべてのデータ取得コマンドは `--json` フラグでJSON出力に対応しており、スクリプトやAIエージェントとの連携に適しています。

## Addnessの構造原理（理想 − 現在 = アクション）

ゴールは「理想の状態」と「現在の状態」のギャップを構造的に埋める仕組みです。

- **完了基準(DoD / `--description`)** = 理想の状態を記述する
- **説明(body)** = 現在の状態を記述する
- **子ゴール** = 理想と現在の差分を埋めるアクションとして分解したもの

この構造を再帰的に適用し、各階層で差分をアクションへ落として実行することで理想を達成します。
AIエージェントとして作業する際は、まずDoD（理想）を確認し、曖昧なら具体化してから差分を子ゴールへ分解してください。

補足:
- ゴールはタイトル名で呼び、IDは補助情報として扱ってください。
- ステータス `CANCELLED` は「中止」ではなく「一時停止」を意味します。親がCANCELLEDでも配下を勝手に動かさないでください。
- タスク・進捗・決定の真実源はAddnessです。ローカルメモではなくゴール／コメントに記録してください。

## 認証

```bash
# ブラウザ経由でログイン（推奨）
addness login

# 設定状態を確認
addness status
addness status --json

# 手動設定（API Key直接入力）
addness configure

# ログアウト
addness logout
```

## 組織管理

```bash
# 所属組織の一覧
addness org list
addness org list --json

# 現在の組織を表示
addness org current
addness org current --json

# 組織を切り替え
addness org switch <ORGANIZATION_ID>
```

## ゴール管理

```bash
# ゴールツリーを一覧表示（デフォルト深さ3）
addness goal list
addness goal list --depth 5
addness goal list --org <ORGANIZATION_ID>
addness goal list --json

# ゴールを作成
addness goal create --title "新しいゴール"
addness goal create --title "サブゴール" --parent <PARENT_GOAL_ID>
addness goal create --title "説明付き" --description "完了条件の説明"
addness goal create --title "新しいゴール" --json

# ゴールの詳細を取得
addness goal get <GOAL_ID>
addness goal get <GOAL_ID> --with-deliverable --with-comment
addness goal get <GOAL_ID> --json

# 子ゴールを取得
addness goal children <GOAL_ID>
addness goal children <GOAL_ID> --limit 50 --offset 0
addness goal children <GOAL_ID> --json

# サブツリーのプレビューを表示（標準では深さ2まで）
addness goal tree <GOAL_ID>
addness goal tree <GOAL_ID> --json

# 兄弟ゴールとその成果物を取得
addness goal siblings <GOAL_ID>
addness goal siblings <GOAL_ID> --limit 50
addness goal siblings <GOAL_ID> --json

# ゴールを検索
addness goal search <KEYWORD>
addness goal search <KEYWORD> --json

# ゴールのステータスやタイトルを更新
addness goal update <GOAL_ID> --status IN_PROGRESS
addness goal update <GOAL_ID> --status COMPLETED
addness goal update <GOAL_ID> --title "新しいタイトル"
addness goal update <GOAL_ID> --description "完了基準"
addness goal update <GOAL_ID> --body "現在の状態"
addness goal update <GOAL_ID> --body-file ./status.md
addness goal update <GOAL_ID> --due-date 2026-07-01
addness goal update <GOAL_ID> --clear-due-date
addness goal update <GOAL_ID> --status NOT_STARTED --title "タイトル変更" --json

# 完了が確認できた範囲だけ、子から親へまとめて完了（全階層・全ページ）
addness goal complete <GOAL_ID> --recursive --dry-run --json
addness goal complete <GOAL_ID> --recursive --json
addness goal complete <GOAL_ID> --recursive --exclude <FEEDBACK_GOAL_ID> --json
# 指定した1件だけを完了（未完了の子があると停止）
addness goal complete <GOAL_ID> --json
```

ステータスの選択肢: `NOT_STARTED`, `IN_PROGRESS`, `COMPLETED`, `CANCELLED`

`goal complete --recursive` は指定ゴールを含み、子一覧の全ページを全階層から取得して子から完了します。
`--exclude` は複数回指定でき、対象の未完了の枝と、それを含む親を未完了のまま残します。
失敗時は非ゼロで終了し、JSONに `completed_ids`・`failed_goal_id`・`error` を返します。
完了済みは再更新しないため、同じコマンドで残りを処理できます。
`--dry-run` は更新せず `planned` に子から親の順で対象を表示します。
`goal tree` / `goal get` の深さ2のプレビューを、配下すべての一覧とみなさないでください。

## コメント

```bash
# ゴールへのコメント一覧
addness comment list --goal <GOAL_ID>
addness comment list --goal <GOAL_ID> --json
addness comment list --goal <GOAL_ID> --resolved false --include-replies

# コメント詳細
addness comment get <COMMENT_ID>
addness comment get <COMMENT_ID> --json

# コメントを投稿
addness comment create --goal <GOAL_ID> --body "進捗報告です"
addness comment create --goal <GOAL_ID> --body "完了しました" --json
addness comment create --goal <GOAL_ID> --body-file ./comment.md
printf "確認お願いします" | addness comment create --goal <GOAL_ID> --body -
addness comment create --goal <GOAL_ID> --parent <COMMENT_ID> --body "返信です"

# 作業完了・確認依頼・ブロック通知を送る（対象ゴールへの通知用コメント + 端末通知）
addness notification send --goal <GOAL_ID> --kind done --body "作業が完了しました"
addness notification send --goal <GOAL_ID> --kind review --body "確認お願いします" --json
printf "完了しました" | addness notification send --goal <GOAL_ID> --kind done --body -
addness notification send --goal <GOAL_ID> --kind review --body "確認お願いします" --mention <ORG_MEMBER_ID>

# コメント編集・削除・解決
addness comment update <COMMENT_ID> --body "更新後の本文"
addness comment delete <COMMENT_ID> --force
addness comment resolve <COMMENT_ID>
```

## 組織

```bash
# 組織作成
addness org create --name "新しい組織" --type PERSONAL
addness org create --name "新しい会社" --type BUSINESS --team-scale 2_5 --switch
```

## PRリンク・進捗記録

```bash
# PRをゴールにリンク（成果物として登録）
addness link pr --goal <GOAL_ID> --url https://github.com/org/repo/pull/42
addness link pr --goal <GOAL_ID> --url <PR_URL> --name "PR #42: 機能追加"
addness link pr --goal <GOAL_ID> --url <PR_URL> --comment "実装完了"
addness link pr --goal <GOAL_ID> --url <PR_URL> --json

# 成果物を追加
addness deliverable add --goal <GOAL_ID> --link-url https://example.com --name "参考リンク"

# 進捗を記録（コメント + オプションでステータス更新）
addness link progress --goal <GOAL_ID> --message "設計レビュー完了"
addness link progress --goal <GOAL_ID> --message "実装完了" --status COMPLETED
addness link progress --goal <GOAL_ID> --message "着手開始" --status IN_PROGRESS --json
```

## 今日のtodo（今日のゴール）

その日に取り組むゴールを「今日のtodo」として読み書きします。

```bash
# 今日のtodoを一覧（サブコマンド省略時のデフォルト）
addness today
addness today list --json
addness today list --date 2026-06-29 --json

# 今日のtodoとしてゴールを追加（--parent 省略でルートゴール）
addness today add --title "設計レビューを終える"
addness today add --title "サブタスク" --parent <PARENT_GOAL_ID> --description "完了基準"

# 完了 / 再オープン / ステータス変更
addness today done <GOAL_ID>
addness today reopen <GOAL_ID>
addness today status <GOAL_ID> IN_PROGRESS
```

## 進捗サマリー

```bash
# ゴール全体の進捗サマリーを表示
addness summary
addness summary --depth 5
addness summary --json
```

## Addness Form

フォームの定義は JSON ファイルから渡します。作成時は `title` が必須です。更新時は最新の `revision` を `--revision` に指定し、ファイル側には含めません。管理用の取得コマンドには `--json` を付けてください。

```bash
addness form list --json
addness form list --goal <GOAL_ID> --query "アンケート" --limit 20 --json
addness form list --trash --json
addness form get <FORM_ID> --json
addness form create --definition-file ./form.json --json
addness form replace <FORM_ID> --revision 1 --definition-file ./form.json --json
addness form patch <FORM_ID> --revision 1 --definition-file ./changes.json --json
addness form publish <FORM_ID> --revision 2 --json
addness form close <FORM_ID> --revision 3 --json
addness form unpublish <FORM_ID> --revision 4 --json
addness form delete <FORM_ID> --json
addness form restore <FORM_ID> --json
addness form purge <FORM_ID> --json
addness form responses <FORM_ID> --limit 50 --json
addness form response <FORM_ID> <RESPONSE_ID> --json
addness form summary <FORM_ID> --json
addness form summary <FORM_ID> --submitted-at-from 2026-09-01T00:00:00+09:00 --submitted-at-before 2026-10-01T00:00:00+09:00 --json
addness form export-csv <FORM_ID> --output ./responses.csv --view labels --json
addness form delete-response <FORM_ID> <RESPONSE_ID> --json
addness form delete-all-responses <FORM_ID> --json
```

`list` と `responses` の結果に `nextCursor` がある場合、同じ絞り込み条件で `--cursor` を渡すと次ページを取得できます。`summary` も提出時刻の範囲で回答数・設問・クイズの集計を絞り込めます。CSV は既存ファイルを上書きしません。フォーム・回答の削除操作には確認が入り、削除後は復元できません。

## ゴール検出

```bash
# 現在のブランチからゴールIDを自動検出
addness detect-goal
addness detect-goal --json
```

ブランチ命名規則: `goal/<GOAL_ID>/description`

例: `goal/19453a2d-6524-4bbb-8e4f-f8fd69f3fce4/add-email-notifications`

## 最新 MCP の全機能を CLI から利用する

CLI の固定サブコマンドにない機能は、公開中の MCP カタログから探して実行できます。
フォーム、Analytics/KPI、CRM、Drive、テンプレート、ナレッジ、録画、権限、エージェント作業などを対象に、サーバー更新で追加されたツールにも対応します。

```bash
addness mcp list --json
addness mcp list --query analytics --json
addness mcp schema list_forms --json
addness mcp call list_forms --args '{"limit":20}' --json
addness mcp call <TOOL_NAME> --args-file ./arguments.json --json
# stdin は --args-file -。個人用キーを使う場合は --scope personal。
```

`--org` は今回の対象組織を指定します。`--scope organization|personal|copilot|openai|admin|support` は接続入口を指定し、それぞれの入口に許可されたキーが必要です。個人用・運営用キーは `ADDNESS_API_TOKEN` で渡せます。権限を昇格させる機能ではありません。
`mcp call` は書き込みも即時実行し、自動再送しません。JSON出力には content / structuredContent / isError を保持し、ツール失敗時は非ゼロ終了します。結果に次ページのカーソルがあれば、各ツールのschemaに従って続きを取得します（mcp listのカタログ自体は全ページ取得します）。

v0.15.0では、廃止された旧個人ドキュメント・旧スキル・Codexジョブ・共有ツリー・請求書・ゴールalias/recurring/duplicate・旧議事録APIのコマンドを削除しました。APIキーを受け付けない招待承諾やキー作成などはWeb画面で行ってください。コメントはv2、成果物とPRリンクは新Driveへ移行しています。`comment list-all` には `--author <ORG_MEMBER_ID>` が必須です。定期実行はテンプレートscheduleまたは個人用ルーティンのMCPツールを確認してください。

## AIエージェント向けガイドライン

### 現行APIと移行先
- `execution generate` は廃止され、バックエンドは `RECURRING_GENERATION_REMOVED` を返します。テンプレートの定期ルーティンは `addness today planned --help` で確認してください。
- `goal-chat` / `todo-chat` / `thread` / `goal decompose` は、現行バックエンドに同じルートがないためCLIから削除されています。ゴール上の会話には `issue` または `comment`、AI診断には `core-values` / `master-plan`を使ってください。旧 `codex-job` も現行バックエンドから削除済みです。エージェント機能は `addness mcp list --query agent --json` で現行ツールを確認してください。いずれも旧スレッドAPIと完全に同じ機能ではありません。ゴール分解APIの後継はないため、子ゴールは `addness goal create --parent <GOAL_ID>` で作成します。
- 旧 `kpi add/update/rm` は廃止しました。現行バックエンドのKPIはAnalytics Recipeを定義し、KPIツリーに配置し、Runで実績を記録する構造です。現行操作は `addness mcp list --query analytics --json` / `--query kpi` で確認し、`mcp schema` / `mcp call` を使ってください。
- 403でレスポンスがJSONではない場合は、画面側と同じくWAF/セキュリティルールによるブロックの可能性があります。OWNER/EDITOR権限の変更では直りません。発生時刻と `Request path` を添えて運用担当に確認してください。
- `org get-context` / `set-context` / `context-revisions` は、backendが組織コンテキスト機能を撤去したためCLIから削除されています。保存内容がAddyの挙動に反映されなかった機能で、現行APIに直接の後継はありません。

### 作業開始時（必須）
1. `addness detect-goal --json` でブランチに紐づくゴールを確認してください。
2. ゴールが検出された場合、`addness goal get <ID> --json --with-deliverable --with-comment` で詳細を確認してから作業を開始してください。
3. ゴールが検出されない場合、`addness goal list --json --depth 3` で全体を確認し、関連するゴールを特定してください。

### DoD（完了基準）の確認と具体化
- 取り組む前に、対象ゴールのDoD（`--description` の内容）が十分かを確認してください。
- 曖昧・不十分なら、人間に質問して具体化し、`addness goal update <ID> --description "..."`（または `--description-file`）で書き戻してください。勝手に確定しないでください。
- 作業環境・現在地・未完了点・次にやることは、コメントではなく `addness goal update <ID> --body "..."`（または `--body-file`）で現状欄に集約してください。
- 理想と現在の差分を埋めるアクションは、子ゴールとして `addness goal create --title "..." --parent <ID>` で分解してください。

### 作業中
- データ取得時は必ず `--json` フラグを使用してください。構造化データとして処理できます。
- 最初に `addness status --json` で認証状態を確認してください。
- 組織が未設定の場合は `addness org list --json` で一覧を取得し、`addness org switch <ID>` で設定してください。
- 決定や進捗は `addness comment create --goal <ID> --body "..."` に記録してください。コメント末尾にはAIであることが分かる署名（例: 「Codexより」）を付け、人間のコメントと区別してください。
- ユーザーへ作業完了通知を送りたい場合は `addness notification send --goal <ID> --kind done --body "..."` を使ってください。確認依頼は `--kind review`、ブロック中は `--kind blocked` です。対象ゴールへ通知用コメントを残し、同時に端末へ BEL/OSC 通知を送ります。TUI codex セッションでは `ADDNESS_GOAL_ID` があるため `--goal` を省略できます。

### 作業完了時
- 作業完了時は `addness link progress --goal <ID> --message "内容" --status COMPLETED` で進捗を記録してください。
- PRを作成した場合は `addness link pr --goal <ID> --url <PR_URL>` でゴールに紐づけてください。
- ゴールの更新後は `addness goal get <ID> --json` で結果を確認してください。
"#;

pub fn handle_skills() -> Result<()> {
    print!("{SKILLS_PROMPT}");
    Ok(())
}
