pub mod activity;
pub mod ai_chat_render;
pub mod api_key;
pub mod assignment;
pub mod chat;
pub mod codex_job;
pub mod comment;
pub mod configure;
pub mod consent;
pub mod core_values;
pub mod deliverable;
pub mod desktop_auth;
pub mod detect;
pub mod diagnosis;
pub mod execution;
pub mod goal;
pub mod goal_chat;
pub mod goal_decompose_render;
pub mod invitation;
pub mod invoice;
pub mod issue;
pub mod kpi;
pub mod link;
pub mod login;
pub mod master_plan;
pub mod media;
pub mod meeting;
pub mod member;
pub mod notification;
pub mod org;
pub mod personal;
pub mod referral;
pub mod search;
pub mod sharetree;
pub mod skill;
pub mod skills;
pub mod streak;
pub mod summary;
pub mod today;
pub mod todo_chat;
pub mod tool;
pub mod update;
pub mod user;

use anyhow::Result;
use std::io::{self, Write};

/// Show a "[y/N]" prompt on stderr (flushed) and return whether the user typed y/Y.
/// Used by all `--force`-gated destructive subcommands so the prompt is reliably
/// visible even when stderr is fully buffered (e.g. piped through other tools).
pub fn confirm(prompt: &str) -> Result<bool> {
    eprint!("{prompt} [y/N] ");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}
