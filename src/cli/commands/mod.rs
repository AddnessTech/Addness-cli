pub mod assignment;
pub mod comment;
pub mod configure;
pub mod deliverable;
pub mod detect;
pub mod goal;
pub mod invitation;
pub mod kpi;
pub mod link;
pub mod login;
pub mod member;
pub mod org;
pub mod skills;
pub mod summary;
pub mod update;

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
