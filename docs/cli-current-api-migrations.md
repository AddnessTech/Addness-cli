# 現行バックエンドに合わせたCLI API移行

確認基準: vision-todo-backend `origin/main` (`b801a8a`, 2026-10-02)。CLIに残っていた経路を現行ルート登録とハンドラで照合した。

| 旧CLI/API | 現行バックエンド | CLIの扱い |
|---|---|---|
| `goal-chat` (`/api/v2/ai-goal-chat/*`) | 旧ルートなし。現行のAI対話は `ai-core-values`、`ai-master-plan`、`ai-current-map`、`ai-onboarding-hearing` の診断モード別。ゴール上の会話は Goal Issue (`/api/v2/objectives/:id/issues`) に分かれている | `goal-chat` を削除。ゴール会話は `issue` / `comment`、CLI対応済みの診断対話は `core-values` / `master-plan` を使う。`current-map` / `onboarding-hearing` はCLI未対応で、旧AIゴールチャットと同一の後継ではない |
| `todo-chat` (`/api/v2/ai-todo-chat/*`) | 旧ルートなし。今日の作業は `/api/v2/organizations/:id/today-todos` と `planned-todos`、定期ルーティンはテンプレートのschedulesで管理 | `todo-chat` を削除。今日の項目は `today`、予定・ルーティンは `today planned` を使う。対話モードの一対一の後継はない |
| `thread` (`/api/v1/team/ai/threads*`) | 旧ルートなし。クラウドCodex作業は `/api/v2/codex/jobs*`、診断対話は前記のモード別ルート | `thread` を削除。Codex作業には `codex-job` を使う。旧スレッドのtrace/share/question/tool-confirmation機能とは契約が異なる |
| `goal decompose` (`/api/v1/objectives/:id/decompose`) | 同等のHTTPルートなし | サブコマンドを削除。子ゴールは `goal create --parent <GOAL_ID>` で作成する |
| `kpi add/update/rm` (`/api/v2/objectives/:id/kpis`, `/api/v2/objective-kpis/:id`) | 旧ゴール直結CRUDは未登録。現行KPIはAnalytics Recipe、`/api/v2/analytics/kpi-tree*`、RecipeのRun/metric-pointsで管理する | 旧 `kpi` コマンドを削除。旧title/unit/target/actual形式とRecipe/Run形式は同等でないため自動変換しない。現行Analytics操作はCLI未対応 |
| `execution generate` (`/api/v2/execute-goals/generate`) | ルート登録は残るがハンドラは410 `RECURRING_GENERATION_REMOVED` を返し、テンプレートのルーティンを案内する | サブコマンドとクライアント関数を削除。予定の作成・採用は `today planned` を使う |
| `org get-context/set-context/context-revisions` (`/api/v2/organizations/:id/context*`) | backend commit `f3ae686d` でAPIと機能を撤去。組織コンテキストはAddyのプロンプトへ注入されず、保存しても挙動に反映されなかった | 3コマンドとAPIクライアントを削除。直接の後継機能はない。古いバックエンドcheckoutやAPI資料にルートが見えても、現行 `origin/main` の仕様ではない |

旧コマンドは互換目的で残すのではなく、現行サーバーにないAPIへリクエストしないようCLIから削除した。機能差がある場合は名称だけで別機能へ読み替えず、上表のように区別する。

## エラー診断

- 403でレスポンスの `Content-Type` がJSONでない場合、Web画面と同じくWAF/セキュリティルールによるブロックとして案内する。HTML本文はエラー出力から省き、OWNER/EDITOR権限を原因として示さない。
- 組織コンテキストAPIの404は現行仕様どおり。backendが機能自体を撤去しており、CLIに対応コマンドや直接の代替機能はない。
