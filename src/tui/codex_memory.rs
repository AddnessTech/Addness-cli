use std::path::PathBuf;

use chrono::Local;

use crate::api::UpdateGoalRequest;

use super::codex_pane;

pub(super) const CODEX_AUTO_RECORD_START: &str = "<!-- addness:codex:auto-record:start -->";
pub(super) const CODEX_AUTO_RECORD_END: &str = "<!-- addness:codex:auto-record:end -->";
pub(super) const CODEX_DECISION_LOG_START: &str = "<!-- addness:codex:decision-log:start -->";
pub(super) const CODEX_DECISION_LOG_END: &str = "<!-- addness:codex:decision-log:end -->";
pub(super) const CODEX_TRACEABILITY_START: &str = "<!-- addness:codex:traceability:start -->";
pub(super) const CODEX_TRACEABILITY_END: &str = "<!-- addness:codex:traceability:end -->";

fn git_status_short(cwd: &str) -> String {
    let output = std::process::Command::new("git")
        .args(["status", "--short"])
        .current_dir(cwd)
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

fn git_branch_name(cwd: &str) -> String {
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let branch = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if branch.is_empty() {
                "(detached)".to_string()
            } else {
                branch
            }
        }
        _ => "(unknown)".to_string(),
    }
}

pub(super) fn codex_work_memo(cwd: &str, session_state: &str, last_prompt: Option<&str>) -> String {
    let cwd_path = PathBuf::from(cwd);
    let diff = codex_pane::git_diff_stat(&cwd_path);
    let status = git_status_short(cwd);
    let branch = git_branch_name(cwd);
    let diff_text = if diff.trim().is_empty() {
        "差分なし".to_string()
    } else {
        diff
    };
    let status_text = if status.trim().is_empty() {
        "差分なし".to_string()
    } else {
        status
    };
    let prompt_text = last_prompt
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "未記録".to_string());

    format!(
        "## Codex自動メモ(機械)\n\
         - 更新: {}\n\
         - セッション: {session_state}\n\
         - 最後の依頼: {prompt_text}\n\
         - 作業フォルダ: {cwd}\n\
         - ブランチ: {branch}\n\
         - 現在地: プロジェクト固有の判断・決定・次の手は `## Codex作業メモ` / `## Codex決定ログ` / `## PR/Release Traceability` へ集約する（この機械メモは自動更新なので編集不要）\n\
         - git status:\n\
         ```text\n\
         {status_text}\n\
         ```\n\
         - git diff --stat HEAD:\n\
         ```text\n\
         {diff_text}\n\
         ```",
        Local::now().format("%Y-%m-%d %H:%M")
    )
}

fn ensure_codex_block(existing: String, start: &str, end: &str, block_body: &str) -> String {
    // codex が body を書き直して不可視マーカーを落としても、見出し（`## ...`）が
    // 残っていれば既存とみなし、空ブロックを重複追加しない。
    let heading = block_body.lines().next().unwrap_or("").trim();
    let has_markers = existing.contains(start) && existing.contains(end);
    let has_heading = !heading.is_empty() && existing.contains(heading);
    if has_markers || has_heading {
        existing
    } else if existing.trim().is_empty() {
        format!("{start}\n{block_body}\n{end}")
    } else {
        format!("{}\n\n{start}\n{block_body}\n{end}", existing.trim_end())
    }
}

pub(super) fn ensure_codex_memory_sections(body: String) -> String {
    let decision_log = "## Codex決定ログ\n\
        - （決定が出たら `YYYY-MM-DD HH:MM - 決定: ... / 理由: ... / 影響: ...` の形で追記）"
        .replace("\n        ", "\n");
    let traceability = "## PR/Release Traceability\n\
        - PR: 未登録\n\
        - tag/release: 未登録\n\
        - CI: 未記録"
        .replace("\n        ", "\n");
    let body = ensure_codex_block(
        body,
        CODEX_DECISION_LOG_START,
        CODEX_DECISION_LOG_END,
        &decision_log,
    );
    ensure_codex_block(
        body,
        CODEX_TRACEABILITY_START,
        CODEX_TRACEABILITY_END,
        &traceability,
    )
}

/// codex 作業メモを既存 body へ統合した body 更新リクエストを作る。
/// 同期・非同期どちらの記録経路もこれを使い、body 合成ロジックを一本化する。
pub(super) fn codex_body_update_request(
    existing_body: Option<&str>,
    record: &str,
) -> UpdateGoalRequest {
    let body = ensure_codex_memory_sections(upsert_codex_auto_record(existing_body, record));
    UpdateGoalRequest {
        status: None,
        completed_at: None,
        title: None,
        description: None,
        body: Some(body),
        due_date: None,
    }
}

pub(super) fn upsert_codex_auto_record(existing: Option<&str>, record: &str) -> String {
    let existing = existing.unwrap_or("").trim();
    let block = format!("{CODEX_AUTO_RECORD_START}\n{record}\n{CODEX_AUTO_RECORD_END}");

    if let (Some(start), Some(end)) = (
        existing.find(CODEX_AUTO_RECORD_START),
        existing.find(CODEX_AUTO_RECORD_END),
    ) {
        let end = end + CODEX_AUTO_RECORD_END.len();
        let mut next = String::new();
        next.push_str(existing[..start].trim_end());
        if !next.is_empty() {
            next.push_str("\n\n");
        }
        next.push_str(&block);
        let tail = existing[end..].trim_start();
        if !tail.is_empty() {
            next.push_str("\n\n");
            next.push_str(tail);
        }
        next
    } else if existing.is_empty() {
        block
    } else {
        format!("{existing}\n\n{block}")
    }
}

pub(super) fn codex_trace_link_label(name: &str, url: Option<&str>) -> Option<String> {
    // URL のパス構造で判定する。`release`/`tag` のベア部分一致は
    // `staging`（s-tag-ing）や `press release` を誤検知するため使わない。
    let haystack = format!("{name} {}", url.unwrap_or("")).to_lowercase();
    let kind = if haystack.contains("/pull/") {
        "PR"
    } else if haystack.contains("/releases")
        || haystack.contains("/tag/")
        || haystack.contains("/tags/")
    {
        "Release"
    } else {
        return None;
    };
    Some(format!("{kind}: {name}"))
}
