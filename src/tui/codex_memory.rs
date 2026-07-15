use std::path::PathBuf;

use chrono::Local;

use crate::api::UpdateGoalRequest;

use super::agent::{self, CodexWorkSummary};

pub(super) const CODEX_AUTO_RECORD_START: &str = "<!-- addness:codex:auto-record:start -->";
pub(super) const CODEX_AUTO_RECORD_END: &str = "<!-- addness:codex:auto-record:end -->";
pub(super) const CODEX_WORK_MEMO_START: &str = "<!-- addness:codex:work-memo:start -->";
pub(super) const CODEX_WORK_MEMO_END: &str = "<!-- addness:codex:work-memo:end -->";
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

fn git_changed_files(cwd: &str) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["diff", "--name-only", "HEAD"])
        .current_dir(cwd)
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(12)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn markdown_list(items: &[String], empty: &str) -> String {
    if items.is_empty() {
        return format!("- {empty}");
    }
    items
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn compact_memo_line(text: &str, max_chars: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for (idx, ch) in compact.chars().enumerate() {
        if idx == max_chars {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

fn resume_hint(summary: Option<&CodexWorkSummary>, prompt_text: &str) -> String {
    if let Some(item) = summary.and_then(|summary| summary.remaining.first()) {
        return format!("未完了点から再開: {}", compact_memo_line(item, 180));
    }
    if summary.is_some_and(|summary| !summary.implemented.is_empty() || !summary.checks.is_empty())
    {
        return "実装/検証サマリを確認し、残課題がなければユーザーの次依頼から続行".to_string();
    }
    if prompt_text != "未記録" {
        return format!(
            "最後の依頼を起点に再開: {}",
            compact_memo_line(prompt_text, 180)
        );
    }
    "Codex作業メモ、差分、子ゴールを確認して再開".to_string()
}

fn codex_summary_markdown(summary: Option<&CodexWorkSummary>, touched_files: &[String]) -> String {
    let Some(summary) = summary else {
        return String::new();
    };
    format!(
        "\n\n### 作業終了サマリ\n\
         実装したこと:\n\
         {}\n\n\
         触ったファイル:\n\
         {}\n\n\
         通したチェック:\n\
         {}\n\n\
         残課題:\n\
         {}",
        markdown_list(&summary.implemented, "未記録"),
        markdown_list(touched_files, "差分なし"),
        markdown_list(&summary.checks, "未記録"),
        markdown_list(&summary.remaining, "未記録").replace("- 未記録", "- なし/未記録")
    )
}

pub(super) fn codex_work_memo(
    cwd: &str,
    session_state: &str,
    last_prompt: Option<&str>,
    summary: Option<&CodexWorkSummary>,
) -> String {
    let cwd_path = PathBuf::from(cwd);
    let diff = agent::git_diff_stat(&cwd_path);
    let status = git_status_short(cwd);
    let branch = git_branch_name(cwd);
    let touched_files = git_changed_files(cwd);
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
    let summary_text = codex_summary_markdown(summary, &touched_files);
    let resume_hint = resume_hint(summary, &prompt_text);

    format!(
        "## Codex自動メモ(機械)\n\
         - 更新: {}\n\
         - セッション: {session_state}\n\
         - 最後の依頼: {prompt_text}\n\
         - 次回の入口: {resume_hint}\n\
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
         ```{summary_text}",
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
    let work_memo = "## Codex作業メモ\n\
        - 現在地: 未記録\n\
        - 次の手: 未記録\n\
        - メモリ運用: このゴール固有の前提・判断・未完了点はここに集約し、Codexの通常memoryへ混ぜない。"
        .replace("\n        ", "\n");
    let decision_log = "## Codex決定ログ\n\
        - （決定が出たら `YYYY-MM-DD HH:MM - 決定: ... / 理由: ... / 影響: ...` の形で追記）"
        .replace("\n        ", "\n");
    let traceability = "## PR/Release Traceability\n\
        - PR: 未登録\n\
        - tag/release: 未登録\n\
        - CI: 未記録"
        .replace("\n        ", "\n");
    let body = ensure_codex_block(body, CODEX_WORK_MEMO_START, CODEX_WORK_MEMO_END, &work_memo);
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

#[cfg(test)]
mod tests {
    use super::{codex_work_memo, resume_hint};
    use crate::tui::agent::CodexWorkSummary;

    #[test]
    fn resume_hint_prefers_remaining_work() {
        let summary = CodexWorkSummary {
            remaining: vec!["実TUIで確認UIを目視検証する".to_string()],
            ..Default::default()
        };

        assert_eq!(
            resume_hint(Some(&summary), "前の依頼"),
            "未完了点から再開: 実TUIで確認UIを目視検証する"
        );
    }

    #[test]
    fn codex_work_memo_includes_resume_entrypoint() {
        let summary = CodexWorkSummary {
            implemented: vec!["Addness開発者指示を軽量化".to_string()],
            checks: vec!["cargo test addness_developer".to_string()],
            remaining: vec!["実TUIで表示密度を確認".to_string()],
        };

        let memo = codex_work_memo(
            ".",
            "turn 3完了",
            Some("Addness in Codexを通常Codex並みに動かす"),
            Some(&summary),
        );

        assert!(memo.contains("- 次回の入口: 未完了点から再開: 実TUIで表示密度を確認"));
        assert!(memo.contains("### 作業終了サマリ"));
        assert!(memo.contains("Addness開発者指示を軽量化"));
    }
}
