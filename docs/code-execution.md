# Filesystem Code Execution

Addness TUI は、Addness の操作定義をモデルへ一括投入せず、ファイルシステム上の
コード API として公開する。モデルは必要な定義だけを探索・読込し、複数操作を1つの
Node.js プログラムとして合成する。

この設計は、Anthropic の
[Code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)
で示された「1ツール1ファイル」の progressive disclosure と、Cloudflare の
[Code Mode](https://blog.cloudflare.com/code-mode-mcp/) が示した固定サイズの探索・実行面を、
Codex / Claude Code が既に持つファイル探索とsandboxed command executionへ適用したもの。
新しい `/code-exec` や大量のMCPツール定義は追加しない。

設計上の基準は次の2例である。Anthropic は Google Drive から Salesforce へデータを
渡す例で150,000 tokensを2,000 tokensへ減らした（98.7%減）。Cloudflare は
2026年2月、API全体を固定の `search()` / `execute()` だけで探索・実行し、初期露出を
約1,000 tokensに抑える Code Mode を公開した。この実装では同じ役割をエージェント既存の
`find` / `rg` とsandboxed command executionに担わせるため、追加ツールschemaはゼロである。
数値は上流の事例であり、Addness上の削減率を保証するものではない。

## データフロー

```text
model ── find / rg ──> 必要な .mjs 定義だけ読む
  │
  └─ Node.js workflow
       ├─ generated module ──> ADDNESS_BIN ... --json ──> Addness API
       ├─ 中間JSONをNode.jsメモリ内で結合・絞込・変換
       └─ 最小の最終結果だけstdout ──> model context
```

APIレスポンスは子プロセスのpipeからNode.jsへ渡り、runtime自身はログ出力しない。
モデルのcontextへ入るのは、作成したworkflowが明示的にstdoutへ出した値だけである。

## 生成物

TUI起動時にClapの正本定義から、次のツリーを `~/.addness/code-api/` へ生成する。

```text
generated/
├── _runtime/
│   └── client.mjs
├── addness/
│   ├── goal/
│   │   ├── get.mjs
│   │   └── update.mjs
│   ├── comment/
│   │   └── list.mjs
│   └── ...
└── manifest.json
```

- `--json` を持つ非対話コマンドだけを公開する。
- 1 leaf commandを1 `.mjs` にし、入力名・必須性・flag種別・説明を同じファイルへ置く。
- `--json` はruntimeが必ず付与し、JSONまたはJSONLをモデルへ表示せずparseする。
- `_json` で終わる引数には、文字列化前のobject / arrayをそのまま渡せる。
- `--force` を持つ操作は `force: true` がなければ実行前に拒否し、対話promptを開かない。
- API token、保存済みcredential、MCP設定のenvは生成物へ書かない。
- runtimeは `spawn(..., shell: false)` を使い、引数をshell文字列へ連結しない。
- 1操作の既定timeoutは120秒、stdout + stderrの上限は64 MiB。

生成モジュールとruntimeのcontent hashが同じなら再生成しない。差分がある時は一時ディレクトリを完成させてから
`generated/` を置換するため、モデルが半端なツリーを読む時間を作らない。

## エージェントへの公開

Codex / Claude Code の全起動経路へ次を渡す。

```text
ADDNESS_CODE_API_ROOT=~/.addness/code-api
ADDNESS_BIN=/absolute/path/to/addness
```

developer instructions はルートと探索規則だけを固定prefixへ置く。個別schemaはpromptへ
載せず、モデルが `find` / `rg -l` で対象を絞ってから必要な `.mjs` だけを読む。
Claude Code には cwd 外の生成ツリーを読めるよう、このルートだけを暗黙の `--add-dir` として
追加する。Codex は通常のsystem readを使い、書込許可まで広げる `--add-dir` は追加しない。

## 実行例

```js
import path from "node:path";
import { pathToFileURL } from "node:url";

const operation = (...parts) => pathToFileURL(path.join(
  process.env.ADDNESS_CODE_API_ROOT,
  "generated",
  "addness",
  ...parts,
)).href;
const { run: getGoal } = await import(operation("goal", "get.mjs"));
const { run: updateGoal } = await import(operation("goal", "update.mjs"));

const goal = await getGoal({ id: process.env.ADDNESS_GOAL_ID });
await updateGoal({ id: goal.id, body: goal.body });
process.stdout.write(JSON.stringify({ updated: goal.id }));
```

`goal` 全体はモデルcontextへ出ず、最後の `{updated: ...}` だけが戻る。単発の小さな操作でも
同じAPIを使えるが、特に一覧の絞込、複数ページ取得、別操作へのデータ受け渡しで効果が大きい。

Code APIの生成失敗、Node.js未導入、またはJSON対応定義が存在しない場合だけ、従来の
`"$ADDNESS_BIN" ... --json` をfallbackとして使う。
