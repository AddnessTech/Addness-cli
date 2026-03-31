# Addness CLI - 設計ドキュメント

## 概要

Addness SaaSの全操作をターミナルから実行するRust製CLI/TUIアプリケーション。
人間のインタラクティブ操作とClaude Code等のAIツールからの自動操作の両方に対応する。

## コアビジョン

**「Addnessに書いた目標を、AIが自動で実行できるようにする」**

```
[人間] Web UIでゴール作成（例:「認証にGoogle OAuth追加」）
  ↓
[Claude Code] addness CLIで未着手タスクを取得
  ↓
[Claude Code] ゴールの内容を読んで実装、PR作成
  ↓
[Claude Code] CLIでゴールを完了報告 + PRリンクをコメント
  ↓
[人間] Web UIで結果を確認、次のゴールへ
```

### AI連携の段階的アプローチ

| Phase | 方式 | 説明 | 安全性 |
|-------|------|------|--------|
| **Phase 1** | 人間起動 | 人間がClaude Codeを起動し「Addnessのタスクやって」と指示 | 高（人間が判断） |
| **Phase 2** | TUI選択実行 | TUIでゴール一覧を見て、選択→Claude Codeに自動で指示が渡る | 高（人間が選択） |
| Phase 3 | 自動実行 | 新タスク検知→自動で実行（既存のAI Delegationと統合） | 要検討 |

Phase 1・2を先に実装する。Phase 3は既にバックエンドのAI Delegation機能がカバーしている領域であり、
安全面のハードルも高いため後回しにする。

### Phase 1: 人間起動ワークフロー

Claude Codeを人間が起動し、自然言語で指示する。CLAUDE.mdにCLIの使い方を記載しておく。

```bash
# 人間がClaude Codeで指示
> Addnessの自分に割り当てられた未着手タスクを確認して、一番上のやつを実装して

# Claude Codeが実行する流れ
addness goals list --assigned-to me --status NOT_STARTED --json   # タスク取得
# → 内容を読んで実装
addness goals update <id> --status IN_PROGRESS --json             # 着手報告
# → コード実装、PR作成
addness comments create --goal <id> --body "PR: https://..." --json  # 結果報告
addness goals update <id> --status COMPLETED --json               # 完了報告
```

### Phase 2: TUI選択実行ワークフロー

TUI上でゴールを選択し、Claude Codeをサブプロセスとして起動する。

```
addness  # TUI起動
┌─ My Goals ──────────────────────────┐
│ > 認証にGoogle OAuth追加  NOT_STARTED │
│   APIレート制限実装      NOT_STARTED │
│   ダッシュボード改善     IN_PROGRESS │
└─────────────────────────────────────┘
 [Enter] 詳細  [x] 実行(Claude Code)  [s] 検索  [q] 終了

# "x" を押すとClaude Codeが起動し、選択したゴールの内容で実装開始
```

## 技術スタック

| カテゴリ | 選定 | 理由 |
|----------|------|------|
| 言語 | Rust | シングルバイナリ配布、TUI性能 |
| CLIフレームワーク | clap | Rustの標準的なCLI引数パーサー |
| TUIフレームワーク | ratatui | Rust TUIのデファクト |
| 非同期ランタイム | tokio | API呼び出し、SSEストリーミング |
| HTTPクライアント | reqwest | tokio対応、Rustの標準的なHTTPクライアント |
| JSON | serde + serde_json | Rustの標準的なシリアライズ |

## 配布方法

npmは使用しない。Rustのシングルバイナリを直接配布する。

### 1. Shell Script インストーラ（メイン）

```bash
curl -fsSL https://cli.addness.app/install.sh | sh
```

- OS/archを自動検出
- GitHub Releasesからバイナリをダウンロード
- `/usr/local/bin/addness` に配置

### 2. Homebrew（Mac/Linux）

```bash
brew install addness/tap/addness
```

- Homebrew tapリポジトリを別途管理
- リリース時にCIでformulaを自動更新

### 対応プラットフォーム

| Target | OS | Arch |
|--------|----|------|
| `aarch64-apple-darwin` | macOS | Apple Silicon |
| `x86_64-apple-darwin` | macOS | Intel |
| `x86_64-unknown-linux-gnu` | Linux | x64 |
| `x86_64-pc-windows-msvc` | Windows | x64 |

## 認証方式

AWS CLI / GitHub CLI と同じ段階的アプローチを採用する。

### Phase 1: API Key（初期リリース）

```bash
# Web UIでAPI Keyを発行 → CLIに設定
addness auth set-key sk-xxxxx

# 環境変数でも可（CI/CD向け）
ADDNESS_API_KEY=sk-xxxxx addness goals list
```

- Web UIにAPI Key発行画面を新設
- バックエンドにAPI Key CRUD + 認証ミドルウェアを追加
- トークンは `~/.addness/credentials.json` に保存（permission 600）
- 期限なし（ユーザーが失効操作するまで有効）

### Phase 2: ブラウザログイン（後日追加）

```bash
addness auth login
→ ブラウザが開く → Clerkでログイン → CLIにセッションが返る
```

- 既存のDesktop Authエンドポイントを拡張
- Clerk Session Tokenを返すように変更
- 短寿命トークン + リフレッシュの仕組み

### 認証の優先順位

1. `--api-key` フラグ（明示指定）
2. `ADDNESS_API_KEY` 環境変数
3. `~/.addness/credentials.json` の保存済みキー

### API Keyの設計

- **フォーマット**: `sk-` プレフィックス + ランダム文字列（例: `sk-abc123def456...`）
- **スコープ**: 組織単位。1つのキーは1つの組織に紐づく
- **権限**: 初期は全操作可能（read/write）。将来的にread-only等のスコープを追加可能
- **保存**: DBにはハッシュのみ保存。生キーは発行時に一度だけ表示

## プロジェクト構造

```
addness-cli/
├── Cargo.toml
├── src/
│   ├── main.rs              # エントリポイント（引数あり→CLI、なし→TUI）
│   ├── api/                  # APIクライアント（共通層）
│   │   ├── mod.rs
│   │   ├── client.rs         # HTTPクライアント、認証、エラーハンドリング
│   │   ├── models.rs         # APIレスポンスの型定義
│   │   ├── goals.rs          # 目標関連API
│   │   ├── search.rs         # 検索API
│   │   ├── comments.rs       # コメントAPI
│   │   ├── organizations.rs  # 組織API
│   │   ├── members.rs        # メンバーAPI
│   │   ├── threads.rs        # AIスレッドAPI
│   │   └── auth.rs           # 認証API
│   ├── cli/                  # CLIインターフェース
│   │   ├── mod.rs
│   │   ├── commands/         # サブコマンド定義
│   │   │   ├── auth.rs       # addness auth {login, set-key, status}
│   │   │   ├── goals.rs      # addness goals {list, get, create, update, search, ...}
│   │   │   ├── org.rs        # addness org {list, switch}
│   │   │   ├── search.rs     # addness search <query>
│   │   │   ├── comments.rs   # addness comments {list, create, ...}
│   │   │   ├── members.rs    # addness members {list, get}
│   │   │   └── ai.rs         # addness ai {threads, chat, ...}
│   │   └── output.rs         # 出力フォーマッタ（テーブル / --json）
│   ├── tui/                  # TUIインターフェース（ratatui）
│   │   ├── mod.rs
│   │   ├── app.rs            # TUIアプリ状態管理
│   │   ├── event.rs          # キーイベントハンドリング
│   │   ├── ui.rs             # レイアウト・描画
│   │   └── views/            # 各画面
│   │       ├── goals.rs      # ゴールツリー表示
│   │       ├── search.rs     # インタラクティブ検索
│   │       └── detail.rs     # ゴール詳細
│   └── config/               # 設定管理
│       ├── mod.rs
│       ├── credentials.rs    # 認証情報の保存・読み込み
│       └── settings.rs       # 組織コンテキスト等の設定
├── install.sh                # インストールスクリプト
└── .github/
    └── workflows/
        └── release.yml       # クロスコンパイル + GitHub Releases + Homebrew更新
```

## CLIコマンド体系

### 認証

```bash
addness auth set-key <key>     # API Keyを設定
addness auth login             # ブラウザログイン（Phase 2）
addness auth status            # 認証状態を表示
addness auth logout            # 認証情報を削除
```

### 組織

```bash
addness org list               # 所属組織一覧
addness org switch <name|id>   # デフォルト組織を切り替え
addness org current            # 現在の組織を表示
```

### ゴール（目標）

```bash
addness goals list                            # ゴール一覧（ルート階層）
addness goals list --assigned-to me           # 自分に割り当てられたゴール
addness goals list --status NOT_STARTED       # ステータスフィルタ
addness goals get <id>                        # ゴール詳細
addness goals create <title>                  # ゴール作成
addness goals create <title> --parent <id>    # 子ゴール作成
addness goals update <id> --status COMPLETED  # ステータス更新
addness goals update <id> --title "新しい名前" # タイトル更新
addness goals delete <id>                     # ゴール削除
addness goals search <query>                  # ゴール検索
addness goals tree <id>                       # サブゴールツリー表示
addness goals children <id>                   # 子ゴール一覧
```

### 検索

```bash
addness search <query>                  # 統合検索（ゴール+コメント+メンバー）
addness search <query> --type goals     # ゴールのみ
addness search <query> --type comments  # コメントのみ
```

### コメント

```bash
addness comments list --goal <id>                    # ゴールのコメント一覧
addness comments create --goal <id> --body "内容"    # コメント作成
```

### メンバー

```bash
addness members list           # メンバー一覧
addness members get <id>       # メンバー詳細
```

### AI

```bash
addness ai threads             # AIスレッド一覧
addness ai chat <thread-id>   # AIチャット送信
```

### TUI

```bash
addness                        # 引数なし → TUIモード起動
```

### 共通フラグ

```bash
--json                # JSON出力（AI連携用）
--org <id>            # 組織を明示指定
--api-key <key>       # API Keyを明示指定
--api-url <url>       # APIベースURLを指定（デフォルト: https://api.addness.app）
```

## AI連携（Claude Code）

### CLAUDE.mdに記載する内容

```markdown
# Addness CLI

`addness` CLI is available for managing goals in Addness.
Always use `--json` flag for structured output.

## Workflow: タスク実行

1. 未着手タスクを確認: `addness goals list --assigned-to me --status NOT_STARTED --json`
2. タスク詳細を読む: `addness goals get <id> --json`
3. 着手報告: `addness goals update <id> --status IN_PROGRESS --json`
4. 実装完了後に報告: `addness comments create --goal <id> --body "PR: <url>" --json`
5. 完了: `addness goals update <id> --status COMPLETED --json`

## Quick Reference

- ゴール検索: `addness goals search "<keyword>" --json`
- ゴールツリー: `addness goals tree <id> --json`
- 統合検索: `addness search "<keyword>" --json`
```

### Claude Codeからの利用イメージ

```bash
# 人間の指示: 「Addnessの自分のタスクを確認して、一番上のやつを実装して」

# Step 1: タスク取得
addness goals list --assigned-to me --status NOT_STARTED --json
# → [{"id": "abc-123", "title": "認証にGoogle OAuth追加", "description": "..."}]

# Step 2: 着手報告
addness goals update abc-123 --status IN_PROGRESS --json

# Step 3: 詳細確認（サブゴールも）
addness goals get abc-123 --json
addness goals children abc-123 --json

# Step 4: 実装 ... (Claude Codeが通常のコーディング)

# Step 5: 結果報告
addness comments create --goal abc-123 --body "実装完了。PR: https://github.com/..." --json
addness goals update abc-123 --status COMPLETED --json
```

## バックエンド変更（必要）

### API Key機能の新設

1. **DBテーブル**: `api_keys`
   - `id` — UUID
   - `organization_id` — 紐づく組織
   - `user_id` — 発行したユーザー
   - `key_prefix` — キーの先頭8文字（UI表示用: `sk-abc1****`）
   - `key_hash` — SHA-256ハッシュ（認証用）
   - `name` — ユーザーが付ける識別名（例: "My CLI Key"）
   - `last_used_at` — 最終使用日時
   - `expires_at` — 有効期限（NULLなら無期限）
   - `created_at`, `revoked_at`

2. **エンドポイント**:
   - `POST /api/v1/team/api-keys` — API Key発行（レスポンスに生キーを含む。一度きり）
   - `GET /api/v1/team/api-keys` — API Key一覧（生キーは含まない）
   - `DELETE /api/v1/team/api-keys/:id` — API Key失効

3. **認証ミドルウェア拡張**:
   - 既存のClerk JWT認証に加えて、`Bearer sk-xxxxx` 形式のAPI Keyも受け付ける
   - `sk-` プレフィックスで判別 → `api_keys` テーブルをハッシュで検索
   - ヒットしたらuser_id/organization_idをコンテキストに設定（Clerk認証と同じ形式）
   - `last_used_at` を更新

## 設定ファイル

### ~/.addness/credentials.json

```json
{
  "api_key": "sk-xxxxx",
  "api_url": "https://api.addness.app"
}
```

### ~/.addness/config.json

```json
{
  "default_organization_id": "org-xxxxx",
  "output_format": "table"
}
```

## 開発ログ

### 2026-03-30: 初期実装完了

以下の機能を実装し、ローカル環境で動作確認済み。

#### 実装済み機能
- **認証**: `addness auth set-token <JWT>`, `auth status`, `auth logout`
  - `~/.addness/credentials.json` にトークンとAPI URLを保存（permission 600）
  - 現時点ではClerk JWTを直接設定する方式（API Key対応は未実装）
- **組織管理**: `addness org list`, `org switch <id>`, `org current`
  - `~/.addness/config.json` にデフォルト組織IDを保存
  - テーブル表示 + `--json` 出力対応
- **ゴール一覧**: `addness goals list [--depth N] [--json]`
  - V2 API (`/api/v2/organizations/:id/objectives/tree`) を使用
  - `X-Organization-ID` ヘッダーを自動付与
  - テーブル表示（ID, タイトル, ステータス, オーナー）+ JSON出力対応

#### 動作確認結果
- バックエンド: Docker Compose（localhost:8080）
- 認証: Clerk Backend API (`/v1/sessions/:id/tokens`) でJWTを発行
- ngrok経由でClerkログイン → セッション確立 → JWT取得 → CLI動作確認
- `addness org list` → 4組織表示 ✅
- `addness goals list` → ゴールツリー表示 ✅
- `addness goals list --json` → JSON出力 ✅

#### 技術的な発見
- APIレスポンスは `{ "data": T, "message": "..." }` のラッパー形式
- 組織一覧は `{ "data": [...] }` で直接配列を返す
- ゴールツリーは `{ "data": { "items": [...] } }` でネストされたオブジェクト
- V2 APIは `X-Organization-ID` ヘッダーが必須（cookieの代わり）
- Clerk JWTは60秒で期限切れ → 実運用にはAPI Key機能が必須

#### リポジトリ
- https://github.com/AddnessTech/Addness-TUI (Private)

---

## リリース計画

### v0.1 — CLI基盤 + 読み書き

最小限だがClaude Code連携に必要な機能を揃える。

- [x] プロジェクトセットアップ（Cargo.toml、CI）
- [x] 認証: `auth set-token`, `auth status`, `auth logout`
- [x] 組織: `org list`, `org switch`, `org current`
- [x] ゴール読み取り: `goals list`（ツリー表示 + JSON出力）
- [x] 共通: `--json` フラグ
- [ ] 認証: `auth set-key`（API Key対応）
- [ ] ゴール読み取り: `goals get`, `goals search`, `goals tree`, `goals children`
- [ ] ゴール書き込み: `goals create`, `goals update` (ステータス更新)
- [ ] コメント: `comments list`, `comments create`（完了報告に必要）
- [ ] 検索: `search`
- [ ] エラーハンドリング改善
- [ ] 配布: GitHub Releases + install.sh
- [ ] バックエンド: API Key CRUD + 認証ミドルウェア

### v0.2 — TUI

- [ ] ratatui TUI基盤
- [ ] ゴールツリービュー
- [ ] インタラクティブ検索
- [ ] ゴール詳細表示

### v0.3 — TUI + Claude Code統合

- [ ] TUIからClaude Codeを起動してゴールを実行
- [ ] 実行結果のリアルタイム表示

### v0.4 — 拡張

- [ ] ブラウザログイン（認証Phase 2）
- [ ] AIスレッド・チャット操作
- [ ] Homebrew tap
- [ ] ゴール削除、アーカイブ等の残り操作
