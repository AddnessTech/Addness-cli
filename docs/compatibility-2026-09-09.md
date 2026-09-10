# CLI / TUI 互換性監査（2026-09-09）

対象: Addness CLI v0.11.0、Codex CLI 0.153.4、Claude Code 2.1.266。
両上流の公式 changelog と npm の `latest` を照合し、実バイナリで検証した。
Claude の検証用最新版は一時ディレクトリへ導入し、既存のユーザーインストールを変更していない。

## 修正と判定

| 項目 | 判定・対応 |
|---|---|
| Codex Astra / reasoning | `model/list` で `gpt-6-astra` と max/ultra を確認し、選択 UI と CLI/app-server の両経路に追加。モデルにより推論強度の対応範囲は異なる |
| Claude permission-mode | 実 `--help` に auto/manual が存在。従来の auto→dontAsk の誤変換を修正し、ワンショット・常駐とも上流の値を送る |
| #190 終了時ドレイン | 子プロセス単位の出力チャネルと stdout/stderr EOF を導入。大量出力、遅れた最終 result、stderr、次ターンの開始順、中断後の旧イベントを回帰検証 |
| #196 ページング履歴 | `thread/list` の opaque cursor を追い、exec/appServer/subagent を明示的に取得。`thread/name/set` へ rename を移行。旧環境ではファイル探索を維持 |
| #118 PR表示名の `/` | リンク成果物の共通作成処理で全角スラッシュ等へ正規化。255文字・NULも処理し、リンクURLは変更しない |
| 追加ディレクトリ | Claude の list/clear と常駐再起動、Codex app-server の writable roots を修正。再起動が必要な設定は予約ターン開始前にも反映 |
| #205 Claude inline MCP の信頼 | `/add-dir` で上流の信頼確認が必要なことを案内。フォルダーを対話起動してユーザーが確認する既存の上流手順を維持。信頼境界を回避する処理は追加しない |
| #209 Claude 背景セッション管理 | `agents --help`、`attach --help`、`logs --help` と公式 CLI reference を確認。独立した上流機能であり、既存の print/stream-json 統合を壊す変更はない。TUI 内の専用操作 UI は今後の機能追加候補。現状は上流 CLI を使用 |
| 依存ライブラリ | `lru` 0.18.0 の RUSTSEC-2026-0253 を確認し、修正版 0.18.2 へ更新。既存 CI と同じ除外条件で cargo audit が成功 |

## 検証方法

通常の `cargo test --locked` は、外部モデル呼び出しを必要としない。
実バイナリの追加検証は次のコマンドで実行する。

```sh
ADDNESS_PROBE_CODEX_BIN=/path/to/codex \
ADDNESS_PROBE_CLAUDE_BIN=/path/to/claude \
cargo test --locked upstream_probe_ -- --ignored --nocapture
```

6件のプローブで、両CLIの使用フラグ、Codex initialize、モデルカタログ、
legacy/paginated履歴の作成・ページング・名前更新・別接続からの再取得、
Claudeの双方向control initializeを確認した。
履歴の実モデル通信はローカルHTTPエラーfixtureへ向け、APIキーとモデル利用料を不要にしている。
ユーザーの実データへの書き込みや既存セッションへの変更は検証に使用しない。

フルのモデル回答品質、Claude の未信頼フォルダーからの実 MCP 実行、
背景セッションの attach/stop/rm 操作は、この自動検証の対象外。

## 自動追随ジョブの運用上の残件

2026-09-08 の Upstream Sync は OpenAI から `401 invalid_api_key` を返されて停止している。
GitHub Actions の `OPENAI_API_KEY` secret を管理者が有効なキーへ更新する必要がある。
今回の互換性プローブは認証不要で、通常の PR CI にも追加したため、この secret の状態に依存しない。
ローカルの認証情報の転用や、別アカウントのキーへの置き換えは行っていない。

## 一次資料

- [Codex CLI changelog](https://learn.chatgpt.com/docs/changelog)
- [Codex app-server protocol](https://learn.chatgpt.com/docs/app-server)
- [Claude Code changelog](https://code.claude.com/docs/en/changelog)
- [Claude Code CLI reference](https://code.claude.com/docs/en/cli-reference)
- [RustSec RUSTSEC-2026-0253](https://rustsec.org/advisories/RUSTSEC-2026-0253.html)
- 実 `codex app-server generate-json-schema --experimental`、`codex model/list`、`claude --help`
