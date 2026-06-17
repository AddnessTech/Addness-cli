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
addness comment create --goal <goal-id> --body "実装を開始しました"
```

プルリクエストを紐付ける:

```bash
addness link pr --goal <goal-id> --url https://github.com/org/repo/pull/42
```

コマンドのヘルプを表示する:

```bash
addness --help
addness goal --help
addness org --help
addness comment --help
addness link --help
```

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
