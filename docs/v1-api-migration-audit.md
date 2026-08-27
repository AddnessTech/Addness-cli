# CLI v1 API migration audit

監査基準日: 2026-08-28

- CLI baseline: `626fed7db40672c3dbaaaa1900dfb00483d64e82`
- backend baseline: `0b07c731ba72ec5278ca45262abb0982e9bb3f89`
- 対象: `src/api/client/**/*.rs` と `src/cli/commands/login.rs` の実行経路
- 判定基準: HTTP method、ID の種類、認可、入出力、side effect が同等であること。名前が似ているだけの v2 は移行先とみなさない。

## 結論

実行経路には v1 URL 文字列が 22 ファイル・96 行残っている。backend の現行 `origin/main` と全件突合した結果、今回さらに移行可能だったのは deprecated comment mutation のうち Goal Issue v2 で表現できる経路だった。

以下は既に v2 化済みで、今回の残存数には含まれない。

- organization list / memberships: `GET /api/v2/organizations/me`
- objective 基本 CRUD・詳細・子取得: `/api/v2/objectives*`
- invitation create: v2 invitation API
- codex jobs: `/api/v2/codex/jobs*`

## 今回の v2-first 移行

旧 `comment` コマンド、TUI、`link pr/progress`、`notification send` は同じ compatibility client を通る。Goal Issue v2 の quote-preview で message ID から `objective_id` と root issue ID を解決し、次の経路を優先する。

| 操作 | 優先する v2 | v1 fallback が必要な場合 |
|---|---|---|
| create | `POST /api/v2/objectives/:id/issues` | 4,000文字超、v2 が保持できない inline mention |
| reply | `POST /api/v2/objectives/:id/issues/:issueId/messages` | 親が Goal Issue ではない、または上記 content 制約 |
| edit root/reply | `PATCH /api/v2/objectives/:id/issues/...` | mention row の変更、既存 mention あり、4,000文字超、Goal Issue 外 |
| delete root/reply | `DELETE /api/v2/objectives/:id/issues/...` | Goal Issue 外、または削除済み ID の legacy retry |
| resolve/unresolve | `PATCH /api/v2/goal-issues/:issueId/resolution` | Goal Issue 外または reply ID（v2 は root 専用） |
| add reaction / users | v2 message reaction routes | Goal Issue 外 |

Goal Issue の root/reply 削除は backend 側で legacy delete use case に委譲され、添付 S3 オブジェクト、通知、返信を含む従来の削除 side effect を維持している。v2 の削除 API は CLI にも `issue delete` / `issue delete-message` として公開する。

## 残存 v1 inventory

| 領域 | route文字列数 | 残す理由 / v2 との差 |
|---|---:|---|
| desktop login / desktop auth / API key | 8 | desktop handoff と API key 管理は v1 のみ |
| user / user settings | 8 | v2 `members` は organization member ID と権限を扱い、global user と同一契約ではない |
| organization / subscription / push token | 11 | create、get、delete、root owner、accessible root、access state、subscription 等に同等 v2 がない |
| legacy invitation accept / plan check | 2 | v1 token と v2 invited-member/token resource は互換でなく、plan check の v2 もない |
| goal search/share/public/alias + decompose | 8 | search は backend が CLI/MCP/Addy 用に v1 維持を明記。share/alias/decompose に同等 v2 がない |
| deliverable | 11 | deliverable CRUD/upload/move/batch の v2 がない |
| comment read + compatibility fallback | 11 | global list、get、context、attachment delete に同等 v2 がない。mutation fallback は上表の契約差だけに限定 |
| legacy AI thread | 1 base path（16操作） | 現行 backend では旧 route 自体が撤去済み。mode 別 v2 AI chat は thread model・SSE 契約が異なる |
| notification settings / email destination | 4 | v2 notification feed/preferences は別の設定軸 |
| raw activity logs | 3 | v2 projection/expand/goal-summary は raw list/summary とレスポンス・ページングが異なる |
| meeting bot jobs | 4 | v2 huddle/recording/minutes は別 lifecycle |
| organization skills | 15 | backend から skill API が撤去済みで v2 後継なし |
| unified search | 1 | v2 goal-index / issue search / member search は分割され、横断結果契約が異なる |
| diagnosis stats | 1 | public aggregate stats の同等 v2 なし |
| referral | 4 | create/list/conversion の同等 v2 なし（同一 list path の分岐を2行として計上） |
| invoice | 1 | 同等 v2 なし |
| public goal-tree share | 1 | backend から route が撤去済みで v2 後継なし |
| public streak token | 1 | authenticated v2 streak API と公開 token contract が異なる |
| personal organization ensure | 1 | idempotent ensure の同等 v2 なし |

## 再監査方法

```bash
rg -n '"/api/v1' src/api/client src/cli/commands/login.rs --glob '*.rs'
```

`legacy_comment_routes_are_centralized_in_the_compatibility_client` テストにより、v1 comment URL を compatibility client 外から再び直接呼ばないことを固定する。
