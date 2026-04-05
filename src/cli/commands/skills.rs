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

## AIエージェント向けガイドライン

- データ取得時は必ず `--json` フラグを使用してください。構造化データとして処理できます。
- 最初に `addness status --json` で認証状態を確認してください。
- 組織が未設定の場合は `addness org list --json` で一覧を取得し、`addness org switch <ID>` で設定してください。
- ゴールの全体像を把握するには `addness goal list --json --depth 5` を使用してください。
- 特定のゴールの詳細を調べるには `addness goal get <ID> --json --with-deliverable --with-comment` を使用してください。
- ゴールの更新後は `addness goal get <ID> --json` で結果を確認してください。
"#;

pub fn handle_skills() -> Result<()> {
    print!("{SKILLS_PROMPT}");
    Ok(())
}
