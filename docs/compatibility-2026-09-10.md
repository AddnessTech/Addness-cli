# CLI・TUI互換性修正（2026-09-10 / v0.12.0）

検証対象: Codex CLI 0.154.0、Claude Code 2.1.267。

## Codexの承認設定

- `on-failure`は廃止されたためF4とスラッシュコマンドの選択肢から除外した。旧指定はエラーと有効な選択肢を表示し、別の権限へ自動変換しない。
- `untrusted`はapp-serverのthread/start・thread/resume・設定変更で維持する。CLIの`-a untrusted`だけでなく`-c approval_policy="untrusted"`も0.154.0では拒否されることを実バイナリで確認した。CLI経路へ切り替える場合はプロセス起動前にF4での明示的な設定選択を案内する。`never`等へ暗黙に変換しない。
- Addnessの承認選択はペイン内の状態。外部Codex設定ファイルに保存された旧`approval_policy`は自動編集しない。`on-failure`やCLIの`untrusted`が残る場合は、現行設定を利用者が選び直す。
- `/mcp-server`は廃止を案内し起動しない。現行ヘルプからも除外した。MCP接続の管理は`/mcp`で行う。

## Codexからの質問

`item/tool/requestUserInput`の質問、選択肢、自由入力を会話に表示する。

- `/answer`で現在の質問を再表示する。
- `/answer 1`または選択肢ラベルで回答する。自由入力が可能な質問では`/answer <回答文>`を使う。
- 複数の質問は順番に回答し、すべて回答すると上流へ送信する。`/answer --cancel`は回答一式を取り消す。
- `isBlocking`を状態欄に反映し、回答待ちの予約ターン開始を抑止する。非同期の質問中は通常入力で作業を続けられる。
- タイムアウトや既定の選択肢による自動回答はしない。`serverRequest/resolved`、ターン終了、中断、切断で古い質問を消す。
- 回答は入力履歴・会話ログへ保存しない。`isSecret`の回答は入力欄で伏字にする。

## Claudeの追加指示

ワンショット・常駐の両方で`--system-prompt-snapshot off`を付ける。同じ会話を再開しても、応答言語やAddnessの追加指示を再構築する。2.1.257より前のCLIがこのフラグを未対応として拒否した場合は、そのフラグを省略して一度だけ再試行する。

実Claude CLIとローカルMessages API fixtureを使い、同一session_idで英語から日本語の指示へ切り替わることを送信内容と結果で確認した。この検証は外部モデルへ送信しない。実アカウントでの追加確認は120秒でタイムアウトしたため、外部サービスを含む確認の成功とは扱わない。

## 自動化とCLIの整理

- Upstream Syncの読み取りは最大3回、2秒・4秒の待機を挟んで再試行する。失敗時の途中出力はJSONへ混ぜず、対象コマンドと最終エラーを残す。stateブランチの通信失敗を「未作成」と誤判定しない。
- 手動起動の`detect_only=true`で検出のみを検証できる。AI分析やstate更新は行わない。
- 定期Security監査のIssue登録に必要な`issues: write`をcargo-auditジョブだけに追加した。
- 通常CIのCodex / Claude固定版を0.154.0 / 2.1.267へ更新した。CLI引数の値と`help <subcommand>`の終了状態、app-serverの承認値を追加検証する。
- AI schedule削除PR #207を最新mainへ更新し、削除後のカバレッジ集計も修正してマージした。

## 検証の入口

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
ADDNESS_PROBE_CODEX_BIN=/path/to/codex ADDNESS_PROBE_CLAUDE_BIN=/path/to/claude cargo test --locked upstream_probe_ -- --ignored
ADDNESS_PROBE_CLAUDE_BIN=/path/to/claude python3 .github/scripts/probe-claude-prompt.py
bash .github/scripts/test-retry-read.sh
```

Upstream Syncの既存`OPENAI_API_KEY`による401は別途キー更新が必要。検出の504対策だけで認証まで復旧したとは扱わない。

参照: [Codex app-server](https://learn.chatgpt.com/docs/app-server)、[Claude再開時のsystem prompt](https://code.claude.com/docs/en/cli-reference#system-prompt-flags-in-resumed-conversations)、[PR #207](https://github.com/AddnessTech/Addness-cli/pull/207)。
