# Addness CLI

<p align="center">
  <img src="assets/addness-cli-demo.gif" alt="Addness CLI デモ" width="900">
</p>

Addness CLI は、ローカルの開発環境・スクリプト・AIコーディングエージェントから Addness を操作するためのターミナルインターフェースです。

コマンドラインから離れることなく、ゴールの確認、進捗の更新、コメントの記入、組織の切り替え、プルリクエストと Addness の紐付けを行えます。

## 主な機能

- ターミナルから Addness のゴールを閲覧・確認する。
- スクリプトやローカルのワークフローからゴールのステータス・進捗を更新する。
- ゴールにコメントを作成する。
- GitHub のプルリクエストを Addness のゴールに紐付ける。
- 組織を切り替える。
- 自動化向けに機械可読な JSON 出力を使う。
- macOS・Linux・Windows 上で単一の Rust バイナリとして動作する。

## インストール

macOS・Linux:

```bash
curl -fsSL https://cli.addness.com/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://cli.addness.com/install.ps1 | iex
```

ソースから:

```bash
git clone https://github.com/AddnessTech/Addness-cli.git
cd Addness-cli
cargo build --release
```

## ログイン

`addness login` を実行し、ブラウザでの認証フローを完了してください。

DNS が制限された環境で `failed to lookup address information` が出る場合は、一時回避として
`ADDNESS_API_RESOLVE` で API ホストの解決先を固定できます:

```bash
ADDNESS_API_RESOLVE=vt.api.addness.com=<api-ip> addness goal get <goal-id>
```

IP は運用環境で変わる可能性があるため、通常は設定しないでください。

## 使い方

自分にアサインされたゴールを一覧表示する:

```bash
addness goal list --assigned-to me --status NOT_STARTED
```

スクリプトやエージェント向けに JSON 出力を使う:

```bash
addness goal list --assigned-to me --status NOT_STARTED --json
```

進捗を更新する:

```bash
addness goal update <goal-id> --status IN_PROGRESS
addness goal update <goal-id> --body "現在の状態や次にやること"
addness goal update <goal-id> --due-date 2026-07-01
addness comment create --goal <goal-id> --body "実装を開始しました"
```

プルリクエストを紐付ける:

```bash
addness link pr --goal <goal-id> --url https://github.com/org/repo/pull/42
```

リンク成果物を追加する:

```bash
addness deliverable add --goal <goal-id> --link-url https://example.com --name "参考リンク"
```

コマンドのヘルプを表示する:

```bash
addness --help
addness goal --help
addness org --help
addness comment --help
addness link --help
```

## CLIコマンド一覧

主なコマンドグループの概要です。各コマンドのフラグとサブコマンドの詳細は
`addness <コマンド> --help` を、AIエージェント向けの網羅的な使い方は
`addness skills` を参照してください。

### 認証・設定

- `login` / `logout` / `status` / `configure` — ブラウザ認証・資格情報と API 設定の管理。
- `update` / `completions` — CLI 自体の更新、シェル補完の生成。

### ゴール・成果物

- `goal` — ゴールの作成・更新・削除・ツリー表示・検索・複製・アーカイブ・公開共有・エイリアス。
  `goal recurring` で定期（繰り返し）スケジュール、`goal report-schedule` で活動レポート配信を管理。
  例: `addness goal recurring get <goal-id>`
- `deliverable` — 成果物（ドキュメント/リンク/ファイル）の管理。例: `addness deliverable list --goal <goal-id>`
- `assignment` / `kpi` — ゴールへのメンバー割り当てと KPI の管理。
- `link` — GitHub PR や URL をゴールに紐付け。例: `addness link pr --goal <goal-id> --url <pr-url>`
- `summary` / `detect-goal` — 全ゴールの進捗サマリ表示、git ブランチ名からのゴール ID 検出。

### コミュニケーション

- `comment` — ゴールコメントの作成・一覧・リアクション・解決管理・グローバルフィード。
  例: `addness comment list-all --json`
- `issue` — Goal Issue（ゴール上の v2 チャットスレッド）とゴールセクションの管理。
  例: `addness issue list --goal <goal-id>`
- `chat` — 組織チャット（DM/グループのルーム・メッセージ・招待・横断検索）。
  例: `addness chat room list`
- `notification` — 通知の一覧・未読数・既読/未読管理・購読チャネル（Slack/Email/LINE/Discord）設定・作業通知送信。
  例: `addness notification count`

### 組織・メンバー

- `org` — 組織の CRUD・切り替え・コンテキスト・タイムゾーン・ロゴ・課金/広告/AIスケジュール設定・サブスクリプション。
  例: `addness org get <org-id>`
- `member` — メンバーの一覧/検索/詳細・管理者権限・ピン留め・タグ・アバター・削除プレビュー。
  例: `addness member browse --limit 20`
- `invitation` — メール招待・招待リンクに加え、受け取る側の確認・承諾・辞退。
  例: `addness invitation pending list`
- `user` — 自分のプロフィール・ユーザー設定・所属組織の管理。例: `addness user me`
- `invoice` — 組織の請求書一覧。

### 実行・計画（今日のタスク）

- `today` — 今日のゴールの読み書きに加え、`today todo`（今日の ToDo 行）、
  `today planned`（予定/定期/バックログの素材プール）、`today calendar`（外部カレンダーとヒートマップ）。
  例: `addness today todo list`
- `execution` — 実行タブのサマリ・実行レコードの生成/更新/履歴・ゴール折りたたみ設定・
  アクティブハドル・Codex エージェントビュー。例: `addness execution summary`
- `activity` — メンバー別/ゴール別/組織全体のアクティビティログと集計。例: `addness activity list`
- `streak` — 連続達成ストリークの確認・フリーズ・復活・公開共有。例: `addness streak get`

### 個人スペース・横断機能

- `personal` — 個人スペース（now/日次ドキュメント、構造化 Markdown 編集、エージェントセッション、
  プロジェクト、組織横断の today 一覧・活動ヒートマップ）。例: `addness personal today`
- `search` — 目標・コメント・メンバーの横断検索。例: `addness search "リリース" --json`
- `diagnosis` — 診断結果（ゴールスタイル・価値観・コアバリュー・マスタープラン）の保存・参照・公開設定。
- `referral` — 紹介リンクの作成と紹介実績の確認。
- `share-tree` — ゴールツリーの持ち運び可能な公開エクスポートとクローン。
- `media` — エディタ内インライン画像/動画のアップロードと表示 URL 取得。

## TUI（ターミナル UI）

サブコマンドなしで起動すると、ゴールツリーを操作できる対話的な TUI が開きます:

```bash
addness
```

主な操作はアプリ内で `?` を押すとヘルプが表示されます。

### TUI 内での codex 連携

ゴール上でアクションメニュー（`o` または `Space`）から **「codexで作業」** を選ぶと、
選択中ゴールの文脈（タイトル・完了基準(DoD)・説明）を渡した状態で
[codex](https://github.com/openai/codex) を `codex exec --json` で起動し、
Addness 側の独自会話ペインに JSONL イベントを表示します。
起動直後は軽量コンテキストだけで即入力でき、実依頼を受けた時に必要に応じて
`addness goal get --json --with-deliverable --with-comment` で対象ゴールを読みます。
最初の実依頼を送ると、TUI は選択中ゴールの直下に Codex 作業用の子ゴールを自動作成し、
以後の `ADDNESS_GOAL_ID` と body 自動記録の対象をその子ゴールへ切り替えます。
この自動作成・自動記録は TUI 本体が API で行うため、Codex のモデル tokens は使いません。
子ゴール作成に失敗した場合は、実行を止めずに選択中ゴールのまま Codex を開始します。
長い運用ルールはチャット本文ではなく `developer_instructions` に渡します。
codex は Addness をその組織/プロジェクト専用の
作業DBとして読み、DoD や子ゴール分解の不足を確認します。Addness への書き込みは、
追加の子ゴール作成やコンテキスト書き込みが必要な時は、軽量/低コストの記録専用
サブエージェントへ委任するよう `developer_instructions` で指示します。
codex は Addness を「タスク DB」として扱い、`addness` CLI 経由で DoD の具体化・
子ゴール作成・進捗コメントを書き戻します。
左の Addness ペインには、対象ゴールのステータス、DoD、子ゴール、コメント数、
Addness への更新ログがライブ表示されます。更新ログは `body`、`DoD`、子ゴール、
コメント/通知、成果物など、どの領域が動いたか分かる文言で出ます。
codex の終了時またはペインを閉じる時には、作業フォルダ・ブランチ・git status・
diff stat が対象ゴールの body に自動記録されます。
codex が作業完了を通知したい時は
`addness notification send --kind done --body "実装が完了しました"` を使えます。
確認依頼は `--kind review`、ブロック中は `--kind blocked` です。TUI から起動した
codex では対象ゴール ID が環境変数で渡されるため、`--goal` は省略できます。
通知は Addness には対象ゴールのコメントとして残し、同時に TUI が動いている端末へ
BEL/OSC で送ります。

`codex exec` は非対話ターン実行のため、Codex TUI そのものの見た目や承認UIは使いません。
Addness 側では会話履歴・スクロール・入力欄を保持し、各入力ごとに 1 ターン実行します。
2 ターン目以降は `codex exec resume <thread_id> --json` で同じ Codex セッションを継続します。
Codex が実行したコマンドやツール実行の開始・終了・出力イベントは、Addness 側の履歴に
`Tool` 行として表示されます。履歴上部には現在状態、表示フィルタ、検索語、thread id、
履歴保存状態を固定表示します。
Codex 起動中は左上の「Codex 作業ダッシュボード」に短い操作一覧が常時表示され、
詳細な一覧は `Ctrl-?` で開けます。
`/goal <目標>` で Addness 側の Goal mode を開始でき、以後の `codex exec` ターンには
その永続目標が注入されます。`/goal` は表示、`/goal pause` / `/goal resume` は一時停止/再開、
`/goal clear` は解除です。`/status` と `/help` で現在状態と slash 一覧を確認できます。
実行中ターンは `Ctrl-C` で中断できます。`exec` モードでは承認待ちを同一ターン内で
Addness 側に差し戻す経路がないため、追加承認が必要な操作は Codex の失敗/エラーイベントとして表示されます。

### エージェント実行状況の可視化

エージェントの実行中は、履歴とステータス行に「今なにをしているか」がライブ表示されます:

- 実行中の直近ツール（コマンド実行・ファイル変更など）を 1 行で常時表示します。
  ファイル変更（FileChange）イベントも「今」表示に即時反映されます。
- 実行中はスピナーで動作中であることを示し、承認待ちになると強調表示に切り替わり、
  応答がないまま放置されると端末へ BEL/OSC で再通知します。
- 恒常的なステータス表示（status_note）と直近の作業アクション（work_action）は分離して
  管理されるため、「直前に何をしたか」と「今何をしているか」が混ざりません。
- 現在の作業対象はゴール階層のパンくずとして表示されます。
- エージェントがサブエージェントを起動した場合は、稼働状況パネルに各サブエージェントの
  状態が一覧表示されます。

codex 終了後は還流バーのキーで成果を Addness に反映できます:

- `c` … 作業差分をプリフィルした進捗コメントを投稿
- `s` … ゴールのステータスを変更
- `d` … 成果物を登録
- `v` … `codex exec`（read-only）で各 DoD 項目の達成可否を自動判定し、契約ペインにチェック
- `Ctrl-?` … codex 起動中の操作方法一覧を表示 / 閉じる
- codex 画面上でのトラックパッド/ホイール … ポインタ下の codex 履歴 / Addness 枠をスクロール
- `Ctrl-T` … codex 履歴の表示を `All` / `Talk` / `Tools` / `Errors` で切り替え
- `Ctrl-F` … codex 履歴検索を開始、`Enter` / `Esc` で検索入力を終了
- `Ctrl-L` … codex 履歴検索を解除
- `Alt-e` … 最新または表示中の turn を開閉
- `Ctrl-O` … 入力可能な状態でも最新の turn を開閉
- `Enter` / `Space` … 履歴を見ている時、codex 終了後、または入力欄が空の時に表示中の turn を開閉
- `1`〜`9` / `Ctrl-1`〜`Ctrl-9` … 履歴を見ている時は数字、いつでも `Ctrl-数字` で指定番号の turn を直接開閉
- `e` … 履歴を見ている時または codex 終了後、表示中の turn を開閉
- `E` / `Ctrl-E` … 古い turn の一括開閉
- `/goal <目標>` … Goal mode を開始し、以後の Codex ターンへ永続目標を注入
- `/goal pause` / `/goal resume` / `/goal clear` … Goal mode の一時停止 / 再開 / 解除
- `/status` / `/help` … Addness側Codexセッション状態 / slash コマンド一覧
- `F12` … 実行中の codex を終了して戻る / `Esc`・`q` … ペインを閉じる

codex 枠の履歴は Addness 側でも `~/.addness/codex-sessions/` に JSONL 保存されます。
表示用ログに加えて raw stdout/stderr イベントも残すため、Codex TUI のスクロールバック量には依存しません。
保存量は既定で最大 20,000 レコードまたは約 20MB です。
画面上では古い行を薄く表示し、取得データやツール出力は行数・文字数・短いプレビューに省略します。
`cargo test` / `cargo clippy` / `cargo build`、`git diff`、Addness の JSON 取得結果は、
生出力より先に結果サマリを表示します。

前提:

- 各ユーザーの環境に [codex](https://github.com/openai/codex) がインストールされ、ログイン済みであること
  （未インストールの場合はその旨を案内し、TUI はクラッシュしません）。
- 別パスの codex を使う場合は環境変数 `ADDNESS_CODEX_BIN` で実行ファイルを指定できます。
- macOS・Linux で動作します（Windows は未検証です）。

## 開発

Addness CLI は Rust で書かれています。

```bash
cargo build
cargo run -- --help
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

## コントリビューション

コントリビューションは GitHub のプルリクエストで歓迎しています。PR を作成する前に、開発環境のセットアップ・レビューの方針・マージのルールについて [CONTRIBUTING.md](CONTRIBUTING.md) を読んでください。

Issue やプルリクエストに、シークレット・ローカル設定・顧客データ・非公開のスクリーンショットを含めないでください。

## セキュリティ

脆弱性は公開の GitHub Issue で報告しないでください。非公開での報告手順については [SECURITY.md](SECURITY.md) を参照してください。

## サポート

再現可能なバグ・機能要望・ドキュメントの問題には GitHub Issues を利用してください。記載すべき内容は [SUPPORT.md](SUPPORT.md) を参照してください。

## ライセンス

Addness CLI は [MIT License](LICENSE) の下で公開されています。

Copyright (c) 2026 Addness.
