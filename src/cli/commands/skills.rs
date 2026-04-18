use anyhow::Result;

const SKILLS_PROMPT: &str = r#"# Addness CLI Skills

## Addnessとは

Addnessは「AIと一緒に人生を進める新しいOS」です。
個人やチームのゴール（目標）をツリー構造で管理し、進捗の可視化・振り返り・コメントによるコミュニケーションを通じて目標達成を支援するプラットフォームです。

Addness CLIはターミナルからAddnessのゴール管理機能を操作するためのコマンドラインツールです。
すべてのデータ取得コマンドは `--json` フラグでJSON出力に対応しており、スクリプトやAIエージェントとの連携に適しています。

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

# サブツリーを表示
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
addness goal update <GOAL_ID> --status NOT_STARTED --title "タイトル変更" --json
```

ステータスの選択肢: `NOT_STARTED`, `IN_PROGRESS`, `COMPLETED`, `CANCELLED`

## コメント

```bash
# ゴールへのコメント一覧
addness comment list --goal <GOAL_ID>
addness comment list --goal <GOAL_ID> --json

# コメントを投稿
addness comment create --goal <GOAL_ID> --body "進捗報告です"
addness comment create --goal <GOAL_ID> --body "完了しました" --json
```

## PRリンク・進捗記録

```bash
# PRをゴールにリンク（成果物として登録）
addness link pr --goal <GOAL_ID> --url https://github.com/org/repo/pull/42
addness link pr --goal <GOAL_ID> --url <PR_URL> --name "PR #42: 機能追加"
addness link pr --goal <GOAL_ID> --url <PR_URL> --comment "実装完了"
addness link pr --goal <GOAL_ID> --url <PR_URL> --json

# 進捗を記録（コメント + オプションでステータス更新）
addness link progress --goal <GOAL_ID> --message "設計レビュー完了"
addness link progress --goal <GOAL_ID> --message "実装完了" --status COMPLETED
addness link progress --goal <GOAL_ID> --message "着手開始" --status IN_PROGRESS --json
```

## 進捗サマリー

```bash
# ゴール全体の進捗サマリーを表示
addness summary
addness summary --depth 5
addness summary --json
```

## ゴール検出

```bash
# 現在のブランチからゴールIDを自動検出
addness detect-goal
addness detect-goal --json
```

ブランチ命名規則: `goal/<GOAL_ID>/description`

例: `goal/19453a2d-6524-4bbb-8e4f-f8fd69f3fce4/add-email-notifications`

## AIエージェント向けガイドライン

### 作業開始時（必須）
1. `addness detect-goal --json` でブランチに紐づくゴールを確認してください。
2. ゴールが検出された場合、`addness goal get <ID> --json --with-deliverable --with-comment` で詳細を確認してから作業を開始してください。
3. ゴールが検出されない場合、`addness goal list --json --depth 3` で全体を確認し、関連するゴールを特定してください。

### 作業中
- データ取得時は必ず `--json` フラグを使用してください。構造化データとして処理できます。
- 最初に `addness status --json` で認証状態を確認してください。
- 組織が未設定の場合は `addness org list --json` で一覧を取得し、`addness org switch <ID>` で設定してください。

### 作業完了時
- 作業完了時は `addness link progress --goal <ID> --message "内容" --status COMPLETED` で進捗を記録してください。
- PRを作成した場合は `addness link pr --goal <ID> --url <PR_URL>` でゴールに紐づけてください。
- ゴールの更新後は `addness goal get <ID> --json` で結果を確認してください。
"#;

pub fn handle_skills() -> Result<()> {
    print!("{SKILLS_PROMPT}");
    Ok(())
}
