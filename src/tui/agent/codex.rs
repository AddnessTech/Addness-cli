//! Codex バックエンド固有ロジック（`agent/mod.rs` から enum ディスパッチで呼ばれる）。
//!
//! ここには純粋関数だけを置く（TUI 状態を持たない）:
//! - `codex exec --json` / `codex` ルートコマンドの起動引数ビルダー
//! - 次ターン設定 `CodexExecSettings` と F2〜F4 で巡回する選択肢 enum 群
//! - `~/.codex` のセッション index / session_meta 探索
//!
//! `CodexPane`（`agent/mod.rs`）側で状態を更新する。

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;

use super::{
    CodexSessionCandidate, addness_tui_developer_instructions, config_override_value,
    split_codex_command_args,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexModelChoice {
    Config,
    Gpt55,
    Gpt5,
    O3,
}

impl CodexModelChoice {
    fn next(self) -> Self {
        match self {
            Self::Config => Self::Gpt55,
            Self::Gpt55 => Self::Gpt5,
            Self::Gpt5 => Self::O3,
            Self::O3 => Self::Config,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Gpt55 => "gpt-5.5",
            Self::Gpt5 => "gpt-5",
            Self::O3 => "o3",
        }
    }

    fn cli_arg(self) -> Option<&'static str> {
        match self {
            Self::Config => None,
            Self::Gpt55 => Some("gpt-5.5"),
            Self::Gpt5 => Some("gpt-5"),
            Self::O3 => Some("o3"),
        }
    }
}

pub(super) fn parse_builtin_model_choice(value: &str) -> Option<CodexModelChoice> {
    match value.to_ascii_lowercase().as_str() {
        "config" | "default" | "clear" => Some(CodexModelChoice::Config),
        "gpt-5.5" | "gpt5.5" | "gpt55" => Some(CodexModelChoice::Gpt55),
        "gpt-5" | "gpt5" => Some(CodexModelChoice::Gpt5),
        "o3" => Some(CodexModelChoice::O3),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexReasoningChoice {
    Config,
    Low,
    Medium,
    High,
    XHigh,
}

impl CodexReasoningChoice {
    fn next(self) -> Self {
        match self {
            Self::Config => Self::Low,
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::XHigh,
            Self::XHigh => Self::Config,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        }
    }

    pub(super) fn config_value(self) -> Option<&'static str> {
        match self {
            Self::Config => None,
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::High => Some("high"),
            Self::XHigh => Some("xhigh"),
        }
    }
}

pub(super) fn parse_reasoning_choice(value: &str) -> Option<CodexReasoningChoice> {
    match value.to_ascii_lowercase().as_str() {
        "config" | "default" | "clear" => Some(CodexReasoningChoice::Config),
        "low" => Some(CodexReasoningChoice::Low),
        "medium" | "med" => Some(CodexReasoningChoice::Medium),
        "high" => Some(CodexReasoningChoice::High),
        "xhigh" | "extra-high" | "extra_high" => Some(CodexReasoningChoice::XHigh),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexApprovalChoice {
    Config,
    Untrusted,
    OnRequest,
    OnFailure,
    Never,
}

impl CodexApprovalChoice {
    fn next(self) -> Self {
        match self {
            Self::Config => Self::Untrusted,
            Self::Untrusted => Self::OnRequest,
            Self::OnRequest => Self::OnFailure,
            Self::OnFailure => Self::Never,
            Self::Never => Self::Config,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Untrusted => "untrusted",
            Self::OnRequest => "on-request",
            Self::OnFailure => "on-failure",
            Self::Never => "never",
        }
    }

    pub(super) fn cli_arg(self) -> Option<&'static str> {
        match self {
            Self::Config => None,
            Self::Untrusted => Some("untrusted"),
            Self::OnRequest => Some("on-request"),
            Self::OnFailure => Some("on-failure"),
            Self::Never => Some("never"),
        }
    }
}

pub(super) fn parse_approval_choice(value: &str) -> Option<CodexApprovalChoice> {
    match value.to_ascii_lowercase().as_str() {
        "config" | "default" | "clear" => Some(CodexApprovalChoice::Config),
        "untrusted" => Some(CodexApprovalChoice::Untrusted),
        "on-request" | "onrequest" => Some(CodexApprovalChoice::OnRequest),
        "on-failure" | "onfailure" => Some(CodexApprovalChoice::OnFailure),
        "never" => Some(CodexApprovalChoice::Never),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexSandboxChoice {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

impl CodexSandboxChoice {
    fn next(self) -> Self {
        match self {
            Self::ReadOnly => Self::WorkspaceWrite,
            Self::WorkspaceWrite => Self::DangerFullAccess,
            Self::DangerFullAccess => Self::ReadOnly,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
        }
    }

    pub(super) fn cli_arg(self) -> &'static str {
        self.label()
    }
}

pub(super) fn parse_sandbox_choice(value: &str) -> Option<CodexSandboxChoice> {
    match value.to_ascii_lowercase().as_str() {
        "read-only" | "readonly" => Some(CodexSandboxChoice::ReadOnly),
        "workspace-write" | "workspace" | "workspacewrite" => {
            Some(CodexSandboxChoice::WorkspaceWrite)
        }
        "danger-full-access" | "danger" | "full" | "dangerfullaccess" => {
            Some(CodexSandboxChoice::DangerFullAccess)
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexLocalProviderChoice {
    Config,
    LmStudio,
    Ollama,
}

impl CodexLocalProviderChoice {
    fn next(self) -> Self {
        match self {
            Self::Config => Self::LmStudio,
            Self::LmStudio => Self::Ollama,
            Self::Ollama => Self::Config,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::LmStudio => "lmstudio",
            Self::Ollama => "ollama",
        }
    }

    fn cli_arg(self) -> Option<&'static str> {
        match self {
            Self::Config => None,
            Self::LmStudio => Some("lmstudio"),
            Self::Ollama => Some("ollama"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexColorChoice {
    Never,
    Auto,
    Always,
}

impl CodexColorChoice {
    fn next(self) -> Self {
        match self {
            Self::Never => Self::Auto,
            Self::Auto => Self::Always,
            Self::Always => Self::Never,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::Auto => "auto",
            Self::Always => "always",
        }
    }
}

pub(super) fn parse_color_choice(value: &str) -> Option<CodexColorChoice> {
    match value.to_ascii_lowercase().as_str() {
        "never" | "off" | "none" => Some(CodexColorChoice::Never),
        "auto" | "default" => Some(CodexColorChoice::Auto),
        "always" | "on" => Some(CodexColorChoice::Always),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexExecSettings {
    pub(super) model: CodexModelChoice,
    pub(super) model_override: Option<String>,
    pub(super) reasoning: CodexReasoningChoice,
    pub(super) approval: CodexApprovalChoice,
    pub(super) sandbox: CodexSandboxChoice,
    pub(super) web_search: bool,
    pub(super) oss: bool,
    pub(super) remote_addr: Option<String>,
    pub(super) remote_auth_token_env: Option<String>,
    pub(super) no_alt_screen: bool,
    pub(super) local_provider: CodexLocalProviderChoice,
    pub(super) profile: Option<String>,
    pub(super) additional_dirs: Vec<String>,
    pub(super) image_paths: Vec<String>,
    pub(super) config_overrides: Vec<String>,
    pub(super) enabled_features: Vec<String>,
    pub(super) disabled_features: Vec<String>,
    pub(super) strict_config: bool,
    pub(super) ignore_user_config: bool,
    pub(super) ignore_rules: bool,
    pub(super) skip_git_repo_check: bool,
    pub(super) ephemeral: bool,
    pub(super) bypass_approvals_and_sandbox: bool,
    pub(super) bypass_hook_trust: bool,
    pub(super) color: CodexColorChoice,
    pub(super) output_schema: Option<String>,
    pub(super) output_last_message: Option<String>,
}

const ADDNESS_MEMORY_CONFIG_OVERRIDES: [&str; 2] = [
    "memories.use_memories=false",
    "memories.generate_memories=false",
];

pub(super) fn default_addness_memory_config_overrides() -> Vec<String> {
    ADDNESS_MEMORY_CONFIG_OVERRIDES
        .iter()
        .map(|value| (*value).to_string())
        .collect()
}

impl Default for CodexExecSettings {
    fn default() -> Self {
        Self {
            model: CodexModelChoice::Config,
            model_override: None,
            reasoning: CodexReasoningChoice::Config,
            approval: CodexApprovalChoice::Config,
            sandbox: CodexSandboxChoice::WorkspaceWrite,
            web_search: false,
            oss: false,
            remote_addr: None,
            remote_auth_token_env: None,
            no_alt_screen: false,
            local_provider: CodexLocalProviderChoice::Config,
            profile: None,
            additional_dirs: Vec::new(),
            image_paths: Vec::new(),
            config_overrides: default_addness_memory_config_overrides(),
            enabled_features: Vec::new(),
            disabled_features: Vec::new(),
            strict_config: false,
            ignore_user_config: false,
            ignore_rules: false,
            skip_git_repo_check: false,
            ephemeral: false,
            bypass_approvals_and_sandbox: false,
            bypass_hook_trust: false,
            color: CodexColorChoice::Never,
            output_schema: None,
            output_last_message: None,
        }
    }
}

impl CodexExecSettings {
    pub fn label(&self) -> String {
        let approval = if self.bypass_approvals_and_sandbox {
            "bypass-all"
        } else {
            self.approval.label()
        };
        let mut parts = vec![
            format!(
                "model:{}",
                self.model_override
                    .as_deref()
                    .unwrap_or_else(|| self.model.label())
            ),
            format!("effort:{}", self.reasoning.label()),
            format!("approval:{approval}"),
            format!("sandbox:{}", self.sandbox.label()),
        ];
        if self.web_search {
            parts.push("search:on".to_string());
        }
        if self.oss {
            parts.push("oss:on".to_string());
        }
        if let Some(remote) = &self.remote_addr {
            parts.push(format!("remote:{remote}"));
        }
        if let Some(env) = &self.remote_auth_token_env {
            parts.push(format!("remote-auth-env:{env}"));
        }
        if self.no_alt_screen {
            parts.push("no-alt-screen".to_string());
        }
        if self.local_provider != CodexLocalProviderChoice::Config {
            parts.push(format!("provider:{}", self.local_provider.label()));
        }
        if let Some(profile) = &self.profile {
            parts.push(format!("profile:{profile}"));
        }
        if !self.additional_dirs.is_empty() {
            parts.push(format!("add-dir:{}", self.additional_dirs.len()));
        }
        if !self.image_paths.is_empty() {
            parts.push(format!("image:{}", self.image_paths.len()));
        }
        if !self.config_overrides.is_empty() {
            parts.push(format!("config:{}", self.config_overrides.len()));
        }
        if !self.enabled_features.is_empty() {
            parts.push(format!("enable:{}", self.enabled_features.len()));
        }
        if !self.disabled_features.is_empty() {
            parts.push(format!("disable:{}", self.disabled_features.len()));
        }
        if self.strict_config {
            parts.push("strict-config".to_string());
        }
        if self.bypass_hook_trust {
            parts.push("bypass-hook-trust".to_string());
        }
        if self.color != CodexColorChoice::Never {
            parts.push(format!("color:{}", self.color.label()));
        }
        if self.output_schema.is_some() {
            parts.push("output-schema".to_string());
        }
        if self.output_last_message.is_some() {
            parts.push("output-last-message".to_string());
        }
        let flags = [
            (self.ignore_user_config, "ignore-user-config"),
            (self.ignore_rules, "ignore-rules"),
            (self.skip_git_repo_check, "skip-git-check"),
            (self.ephemeral, "ephemeral"),
        ]
        .into_iter()
        .filter_map(|(enabled, label)| enabled.then_some(label))
        .collect::<Vec<_>>();
        if !flags.is_empty() {
            parts.push(format!("flags:{}", flags.join(",")));
        }
        parts.join(" ")
    }

    pub(super) fn memory_mode_label(&self) -> String {
        let use_memories = self.config_override_value_for("memories.use_memories");
        let generate = self.config_override_value_for("memories.generate_memories");
        match (use_memories, generate) {
            (Some("false"), Some("false")) => "Addness DB / Codex memory off".to_string(),
            (Some("true"), Some("true")) => "Codex global memory on".to_string(),
            (Some(use_memories), Some(generate)) => {
                format!("Codex memory use={use_memories} generate={generate}")
            }
            (Some(use_memories), None) => {
                format!("Codex memory use={use_memories} generate=config")
            }
            (None, Some(generate)) => {
                format!("Codex memory use=config generate={generate}")
            }
            (None, None) => "Codex memory config".to_string(),
        }
    }

    pub(super) fn memory_mode_is_addness_safe(&self) -> bool {
        self.config_override_value_for("memories.use_memories") == Some("false")
            && self.config_override_value_for("memories.generate_memories") == Some("false")
    }

    fn config_override_value_for(&self, key: &str) -> Option<&str> {
        self.config_overrides
            .iter()
            .find_map(|entry| config_override_value(entry, key))
    }

    pub(super) fn cycle_model(&mut self) -> &'static str {
        self.model = self.model.next();
        self.model.label()
    }

    pub(super) fn model_cli_arg(&self) -> Option<&str> {
        self.model_override
            .as_deref()
            .or_else(|| self.model.cli_arg())
    }

    pub(super) fn cycle_reasoning(&mut self) -> &'static str {
        self.reasoning = self.reasoning.next();
        self.reasoning.label()
    }

    pub(super) fn cycle_approval(&mut self) -> &'static str {
        self.approval = self.approval.next();
        self.approval.label()
    }

    pub(super) fn cycle_sandbox(&mut self) -> &'static str {
        self.sandbox = self.sandbox.next();
        self.sandbox.label()
    }

    pub(super) fn toggle_web_search(&mut self) -> bool {
        self.web_search = !self.web_search;
        self.web_search
    }

    pub(super) fn toggle_oss(&mut self) -> bool {
        self.oss = !self.oss;
        self.oss
    }

    pub(super) fn cycle_local_provider(&mut self) -> &'static str {
        self.local_provider = self.local_provider.next();
        self.local_provider.label()
    }

    pub(super) fn cycle_color(&mut self) -> &'static str {
        self.color = self.color.next();
        self.color.label()
    }
}

pub(super) fn codex_named_subcommand_args(name: &str, raw_args: &str) -> Result<Vec<String>> {
    let mut parsed = split_codex_command_args(raw_args)?;
    match name {
        "doctor" => {
            let mut args = vec!["doctor".to_string()];
            args.append(&mut parsed);
            Ok(args)
        }
        "features" => {
            let mut args = vec!["features".to_string()];
            if parsed.is_empty() {
                args.push("list".to_string());
            } else {
                args.append(&mut parsed);
            }
            Ok(args)
        }
        "mcp" => codex_command_with_default("mcp", "list", parsed),
        "plugin" => codex_command_with_default("plugin", "list", parsed),
        "cloud" => codex_command_with_default("cloud", "list", parsed),
        "debug" => codex_command_with_default("debug", "models", parsed),
        "login" => codex_command_with_default("login", "status", parsed),
        "help" => codex_command_with_args("help", parsed),
        "version" => Ok(vec!["--version".to_string()]),
        "logout" | "update" | "app" | "completion" => codex_command_with_args(name, parsed),
        "sandbox" | "mcp-server" | "exec-server" => codex_command_with_help_default(name, parsed),
        "app-server" => {
            let default = vec![
                "app-server".to_string(),
                "daemon".to_string(),
                "version".to_string(),
            ];
            codex_command_with_vec_default(default, parsed)
        }
        "remote-control" => codex_command_with_help_default("remote-control", parsed),
        "review" => {
            let mut args = vec!["review".to_string()];
            args.append(&mut parsed);
            Ok(args)
        }
        "exec-review" => {
            let mut args = vec![
                "exec".to_string(),
                "review".to_string(),
                "--json".to_string(),
            ];
            args.append(&mut parsed);
            Ok(args)
        }
        "apply" => {
            if parsed.is_empty() {
                anyhow::bail!("apply には task id を指定してください");
            }
            let mut args = vec!["apply".to_string()];
            args.append(&mut parsed);
            Ok(args)
        }
        _ => anyhow::bail!("unsupported codex subcommand alias: {name}"),
    }
}

fn codex_command_with_args(name: &str, mut parsed: Vec<String>) -> Result<Vec<String>> {
    let mut args = vec![name.to_string()];
    args.append(&mut parsed);
    Ok(args)
}

fn codex_command_with_default(
    name: &str,
    default_subcommand: &str,
    mut parsed: Vec<String>,
) -> Result<Vec<String>> {
    let mut args = vec![name.to_string()];
    if parsed.is_empty() {
        args.push(default_subcommand.to_string());
    } else {
        args.append(&mut parsed);
    }
    Ok(args)
}

fn codex_command_with_help_default(name: &str, mut parsed: Vec<String>) -> Result<Vec<String>> {
    let mut args = vec![name.to_string()];
    if parsed.is_empty() {
        args.push("--help".to_string());
    } else {
        args.append(&mut parsed);
    }
    Ok(args)
}

fn codex_command_with_vec_default(
    default_args: Vec<String>,
    mut parsed: Vec<String>,
) -> Result<Vec<String>> {
    if parsed.is_empty() {
        Ok(default_args)
    } else {
        let mut args = vec![default_args[0].clone()];
        args.append(&mut parsed);
        Ok(args)
    }
}

pub(super) fn codex_named_subcommand_args_with_settings(
    mut args: Vec<String>,
    settings: &CodexExecSettings,
) -> Vec<String> {
    if matches!(args.first().map(String::as_str), Some("--version" | "help")) {
        return args;
    }
    let mut out = Vec::new();
    if let Some(remote) = &settings.remote_addr {
        out.push("--remote".to_string());
        out.push(remote.clone());
    }
    if let Some(env) = &settings.remote_auth_token_env {
        out.push("--remote-auth-token-env".to_string());
        out.push(env.clone());
    }
    if settings.strict_config {
        out.push("--strict-config".to_string());
    }
    for config in &settings.config_overrides {
        out.push("-c".to_string());
        out.push(config.clone());
    }
    for feature in &settings.enabled_features {
        out.push("--enable".to_string());
        out.push(feature.clone());
    }
    for feature in &settings.disabled_features {
        out.push("--disable".to_string());
        out.push(feature.clone());
    }
    if codex_command_needs_addness_developer_instructions(&args) {
        push_addness_developer_instructions(&mut out);
    }
    out.append(&mut args);
    out
}

fn codex_command_needs_addness_developer_instructions(args: &[String]) -> bool {
    matches!(
        codex_command_name(args),
        Some("exec" | "review" | "resume" | "fork")
    )
}

fn codex_command_name(args: &[String]) -> Option<&str> {
    let mut index = 0usize;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--" {
            return args.get(index + 1).map(String::as_str);
        }
        if !arg.starts_with('-') || arg == "-" {
            return Some(arg);
        }
        index += 1;
        if codex_global_option_takes_value(arg) {
            index += 1;
        }
    }
    None
}

fn codex_global_option_takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "-a" | "--ask-for-approval"
            | "-s"
            | "--sandbox"
            | "-m"
            | "--model"
            | "-p"
            | "--profile"
            | "-C"
            | "--cd"
            | "-c"
            | "--config"
            | "-i"
            | "--image"
            | "-o"
            | "--output-last-message"
            | "--remote"
            | "--remote-auth-token-env"
            | "--local-provider"
            | "--add-dir"
            | "--enable"
            | "--disable"
            | "--output-schema"
    )
}

pub(super) fn codex_command_category(args: &[String]) -> &'static str {
    match codex_command_name(args) {
        Some("exec") => "agent",
        Some("review") | Some("apply") | Some("sandbox") => "workspace",
        Some("resume" | "fork" | "archive" | "delete" | "unarchive") => "session",
        Some("login" | "logout") => "auth",
        Some("mcp" | "plugin" | "features" | "debug" | "doctor" | "completion" | "update") => {
            "config"
        }
        Some("cloud") => "cloud",
        Some("app" | "app-server" | "remote-control" | "mcp-server" | "exec-server") => "server",
        _ => "codex",
    }
}

pub(super) fn codex_home_dir() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
}

pub(super) fn load_codex_session_candidates(limit: usize) -> Result<Vec<CodexSessionCandidate>> {
    let Some(home) = codex_home_dir() else {
        return Ok(Vec::new());
    };
    load_codex_session_candidates_from(&home, limit)
}

pub(super) fn codex_skill_roots(cwd: &str) -> Vec<PathBuf> {
    let mut roots = vec![
        Path::new(cwd).join(".codex").join("skills"),
        Path::new(cwd).join(".agents").join("skills"),
    ];
    if let Some(home) = codex_home_dir() {
        roots.push(home.join("skills"));
    }
    roots
}

pub(super) fn load_codex_session_candidates_from(
    codex_home: &Path,
    limit: usize,
) -> Result<Vec<CodexSessionCandidate>> {
    let mut sessions = read_codex_session_index(codex_home)?;
    let sessions_dir = codex_home.join("sessions");
    if sessions_dir.is_dir() {
        read_codex_session_meta_files(&sessions_dir, &mut sessions)?;
    }
    let mut values = sessions.into_values().collect::<Vec<_>>();
    values.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.title.cmp(&b.title))
    });
    values.truncate(limit);
    Ok(values)
}

pub(super) fn append_codex_session_rename(session_id: &str, title: &str) -> Result<()> {
    let Some(home) = codex_home_dir() else {
        anyhow::bail!("Codex home を解決できません");
    };
    append_codex_session_rename_to(&home, session_id, title)
}

pub(super) fn append_codex_session_rename_to(
    codex_home: &Path,
    session_id: &str,
    title: &str,
) -> Result<()> {
    fs::create_dir_all(codex_home)?;
    let path = codex_home.join("session_index.jsonl");
    let record = serde_json::json!({
        "id": session_id,
        "thread_name": title,
        "updated_at": chrono::Utc::now().to_rfc3339(),
    });
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(&record)?)?;
    Ok(())
}

fn read_codex_session_index(codex_home: &Path) -> Result<HashMap<String, CodexSessionCandidate>> {
    let path = codex_home.join("session_index.jsonl");
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(e.into()),
    };
    let mut sessions = HashMap::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<CodexSessionIndexRecord>(&line) {
            let title = record
                .thread_name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "untitled".to_string());
            sessions.insert(
                record.id.clone(),
                CodexSessionCandidate {
                    id: record.id,
                    title,
                    updated_at: record.updated_at.unwrap_or_default(),
                    cwd: None,
                },
            );
        }
    }
    Ok(sessions)
}

fn read_codex_session_meta_files(
    dir: &Path,
    sessions: &mut HashMap<String, CodexSessionCandidate>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            read_codex_session_meta_files(&path, sessions)?;
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(candidate) = read_codex_session_meta_file(&path)? else {
            continue;
        };
        sessions
            .entry(candidate.id.clone())
            .and_modify(|existing| {
                if existing.title == "untitled" && candidate.title != "untitled" {
                    existing.title = candidate.title.clone();
                }
                if existing.updated_at.is_empty() {
                    existing.updated_at = candidate.updated_at.clone();
                }
                if existing.cwd.is_none() {
                    existing.cwd = candidate.cwd.clone();
                }
            })
            .or_insert(candidate);
    }
    Ok(())
}

fn read_codex_session_meta_file(path: &Path) -> Result<Option<CodexSessionCandidate>> {
    let file = File::open(path)?;
    let mut lines = BufReader::new(file).lines();
    let Some(line) = lines.next().transpose()? else {
        return Ok(None);
    };
    Ok(parse_codex_session_meta_line(&line))
}

fn parse_codex_session_meta_line(line: &str) -> Option<CodexSessionCandidate> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    let payload = value.get("payload")?;
    let id = payload.get("id").and_then(Value::as_str)?.to_string();
    let updated_at = payload
        .get("timestamp")
        .or_else(|| value.get("timestamp"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_string);
    let title = payload
        .get("thread_name")
        .or_else(|| payload.get("title"))
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("untitled")
        .to_string();
    Some(CodexSessionCandidate {
        id,
        title,
        updated_at,
        cwd,
    })
}

#[derive(Debug, Deserialize)]
struct CodexSessionIndexRecord {
    id: String,
    #[serde(default)]
    thread_name: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
}

pub(super) enum CodexConfigKey {
    DeveloperInstructions,
    ModelReasoningEffort,
}

impl CodexConfigKey {
    fn as_str(&self) -> &'static str {
        match self {
            CodexConfigKey::DeveloperInstructions => "developer_instructions",
            CodexConfigKey::ModelReasoningEffort => "model_reasoning_effort",
        }
    }
}

pub(super) fn codex_config_arg(key: CodexConfigKey, value: &str) -> String {
    format!("{}={}", key.as_str(), toml_basic_string(value))
}

fn push_global_exec_settings(
    args: &mut Vec<String>,
    settings: &CodexExecSettings,
    include_sandbox: bool,
) {
    if let Some(remote) = &settings.remote_addr {
        args.push("--remote".to_string());
        args.push(remote.clone());
    }
    if let Some(env) = &settings.remote_auth_token_env {
        args.push("--remote-auth-token-env".to_string());
        args.push(env.clone());
    }
    if settings.no_alt_screen {
        args.push("--no-alt-screen".to_string());
    }
    if settings.bypass_approvals_and_sandbox {
        args.push("--dangerously-bypass-approvals-and-sandbox".to_string());
    } else if let Some(approval) = settings.approval.cli_arg() {
        // `-a` is a global Codex option. `codex exec -a ...` is rejected, so keep it before `exec`.
        args.push("-a".to_string());
        args.push(approval.to_string());
    }
    if settings.bypass_hook_trust {
        args.push("--dangerously-bypass-hook-trust".to_string());
    }
    if settings.strict_config {
        args.push("--strict-config".to_string());
    }

    if include_sandbox
        && !settings.bypass_approvals_and_sandbox
        && settings.sandbox != CodexSandboxChoice::WorkspaceWrite
    {
        args.push("-s".to_string());
        args.push(settings.sandbox.cli_arg().to_string());
    }
    if settings.web_search {
        args.push("--search".to_string());
    }
    if settings.oss {
        args.push("--oss".to_string());
    }
    if let Some(provider) = settings.local_provider.cli_arg() {
        args.push("--local-provider".to_string());
        args.push(provider.to_string());
    }
    if let Some(profile) = &settings.profile {
        args.push("-p".to_string());
        args.push(profile.clone());
    }
    for dir in &settings.additional_dirs {
        args.push("--add-dir".to_string());
        args.push(dir.clone());
    }
    for config in &settings.config_overrides {
        args.push("-c".to_string());
        args.push(config.clone());
    }
    for feature in &settings.enabled_features {
        args.push("--enable".to_string());
        args.push(feature.clone());
    }
    for feature in &settings.disabled_features {
        args.push("--disable".to_string());
        args.push(feature.clone());
    }
}

fn push_optional_exec_settings(args: &mut Vec<String>, settings: &CodexExecSettings) {
    if settings.ignore_user_config {
        args.push("--ignore-user-config".to_string());
    }
    if settings.ignore_rules {
        args.push("--ignore-rules".to_string());
    }
    if settings.skip_git_repo_check {
        args.push("--skip-git-repo-check".to_string());
    }
    if settings.ephemeral {
        args.push("--ephemeral".to_string());
    }
    if let Some(model) = settings.model_cli_arg() {
        args.push("-m".to_string());
        args.push(model.to_string());
    }
    if let Some(reasoning) = settings.reasoning.config_value() {
        args.push("-c".to_string());
        args.push(codex_config_arg(
            CodexConfigKey::ModelReasoningEffort,
            reasoning,
        ));
    }
    for image in &settings.image_paths {
        args.push("-i".to_string());
        args.push(image.clone());
    }
    if let Some(schema) = &settings.output_schema {
        args.push("--output-schema".to_string());
        args.push(schema.clone());
    }
    if let Some(path) = &settings.output_last_message {
        args.push("-o".to_string());
        args.push(path.clone());
    }
}

pub(super) fn codex_exec_resume_args(
    session_id: Option<&str>,
    use_last: bool,
    include_all: bool,
    prompt: &str,
    settings: &CodexExecSettings,
) -> Vec<String> {
    let developer_instructions = codex_config_arg(
        CodexConfigKey::DeveloperInstructions,
        addness_tui_developer_instructions(),
    );
    let mut args = Vec::new();
    push_global_exec_settings(&mut args, settings, true);
    args.extend(["exec", "resume", "--json"].into_iter().map(str::to_string));
    push_optional_exec_settings(&mut args, settings);
    args.push("-c".to_string());
    args.push(developer_instructions);
    if include_all {
        args.push("--all".to_string());
    }
    if use_last {
        args.push("--last".to_string());
    } else if let Some(session_id) = session_id {
        args.push(session_id.to_string());
    }
    args.push(prompt.to_string());
    args
}

fn push_root_interactive_settings(
    args: &mut Vec<String>,
    cwd: &str,
    settings: &CodexExecSettings,
    force_no_alt_screen: bool,
) {
    push_global_exec_settings(args, settings, true);
    if force_no_alt_screen && !args.iter().any(|arg| arg == "--no-alt-screen") {
        args.push("--no-alt-screen".to_string());
    }
    args.push("-C".to_string());
    args.push(cwd.to_string());
    if let Some(model) = settings.model_cli_arg() {
        args.push("-m".to_string());
        args.push(model.to_string());
    }
    if let Some(reasoning) = settings.reasoning.config_value() {
        args.push("-c".to_string());
        args.push(codex_config_arg(
            CodexConfigKey::ModelReasoningEffort,
            reasoning,
        ));
    }
    for image in &settings.image_paths {
        args.push("-i".to_string());
        args.push(image.clone());
    }
}

fn push_addness_developer_instructions(args: &mut Vec<String>) {
    args.push("-c".to_string());
    args.push(codex_config_arg(
        CodexConfigKey::DeveloperInstructions,
        addness_tui_developer_instructions(),
    ));
}

pub(super) fn codex_root_interactive_args(
    prompt: &str,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Vec<String> {
    let mut args = Vec::new();
    push_root_interactive_settings(&mut args, cwd, settings, true);
    push_addness_developer_instructions(&mut args);
    if !prompt.is_empty() {
        args.push(prompt.to_string());
    }
    args
}

pub(super) fn codex_root_resume_args(
    session_id: Option<&str>,
    use_last: bool,
    include_all: bool,
    include_non_interactive: bool,
    prompt: &str,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Vec<String> {
    let mut args = Vec::new();
    push_root_interactive_settings(&mut args, cwd, settings, true);
    push_addness_developer_instructions(&mut args);
    args.push("resume".to_string());
    if include_all {
        args.push("--all".to_string());
    }
    if include_non_interactive {
        args.push("--include-non-interactive".to_string());
    }
    if use_last {
        args.push("--last".to_string());
    } else if let Some(session_id) = session_id {
        args.push(session_id.to_string());
    }
    if !prompt.is_empty() {
        args.push(prompt.to_string());
    }
    args
}

pub(super) fn codex_root_session_command_args(
    command_name: &str,
    raw_args: &str,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Result<Vec<String>> {
    let mut parsed = split_codex_command_args(raw_args)?;
    let mut args = Vec::new();
    push_root_interactive_settings(&mut args, cwd, settings, true);
    push_addness_developer_instructions(&mut args);
    args.push(command_name.to_string());
    args.append(&mut parsed);
    Ok(args)
}

pub(super) fn codex_fork_args(
    session_id: Option<&str>,
    use_last: bool,
    include_all: bool,
    prompt: &str,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Vec<String> {
    let mut args = Vec::new();
    push_root_interactive_settings(&mut args, cwd, settings, true);
    push_addness_developer_instructions(&mut args);
    args.push("fork".to_string());
    if include_all {
        args.push("--all".to_string());
    }
    if use_last {
        args.push("--last".to_string());
    } else if let Some(session_id) = session_id {
        args.push(session_id.to_string());
    }
    if !prompt.is_empty() {
        args.push(prompt.to_string());
    }
    args
}

pub(super) fn codex_session_admin_args(
    command_name: &str,
    session: &str,
    force: bool,
    mut extra_args: Vec<String>,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Vec<String> {
    let mut args = Vec::new();
    push_root_interactive_settings(&mut args, cwd, settings, false);
    args.push(command_name.to_string());
    if force {
        args.push("--force".to_string());
    }
    args.push(session.to_string());
    args.append(&mut extra_args);
    args
}

pub(super) fn codex_review_args(
    raw_args: &str,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Result<Vec<String>> {
    let mut parsed = split_codex_command_args(raw_args)?;
    let developer_instructions = codex_config_arg(
        CodexConfigKey::DeveloperInstructions,
        addness_tui_developer_instructions(),
    );
    let mut args = Vec::new();
    push_root_interactive_settings(&mut args, cwd, settings, false);
    args.push("review".to_string());
    args.push("-c".to_string());
    args.push(developer_instructions);
    args.append(&mut parsed);
    Ok(args)
}

pub(super) fn codex_apply_args(
    raw_args: &str,
    settings: &CodexExecSettings,
) -> Result<Vec<String>> {
    let mut parsed = split_codex_command_args(raw_args)?;
    if parsed.is_empty() {
        anyhow::bail!("apply には task id を指定してください");
    }
    let mut args = Vec::new();
    for config in &settings.config_overrides {
        args.push("-c".to_string());
        args.push(config.clone());
    }
    for feature in &settings.enabled_features {
        args.push("--enable".to_string());
        args.push(feature.clone());
    }
    for feature in &settings.disabled_features {
        args.push("--disable".to_string());
        args.push(feature.clone());
    }
    args.push("apply".to_string());
    args.append(&mut parsed);
    Ok(args)
}

fn push_optional_exec_review_settings(args: &mut Vec<String>, settings: &CodexExecSettings) {
    if settings.ignore_user_config {
        args.push("--ignore-user-config".to_string());
    }
    if settings.ignore_rules {
        args.push("--ignore-rules".to_string());
    }
    if settings.skip_git_repo_check {
        args.push("--skip-git-repo-check".to_string());
    }
    if settings.ephemeral {
        args.push("--ephemeral".to_string());
    }
    if let Some(model) = settings.model_cli_arg() {
        args.push("-m".to_string());
        args.push(model.to_string());
    }
    if let Some(reasoning) = settings.reasoning.config_value() {
        args.push("-c".to_string());
        args.push(codex_config_arg(
            CodexConfigKey::ModelReasoningEffort,
            reasoning,
        ));
    }
    if let Some(schema) = &settings.output_schema {
        args.push("--output-schema".to_string());
        args.push(schema.clone());
    }
    if let Some(path) = &settings.output_last_message {
        args.push("-o".to_string());
        args.push(path.clone());
    }
}

pub(super) fn codex_exec_review_args(
    raw_args: &str,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Result<Vec<String>> {
    let mut parsed = split_codex_command_args(raw_args)?;
    let developer_instructions = codex_config_arg(
        CodexConfigKey::DeveloperInstructions,
        addness_tui_developer_instructions(),
    );
    let mut args = Vec::new();
    push_global_exec_settings(&mut args, settings, true);
    args.push("-C".to_string());
    args.push(cwd.to_string());
    args.extend(["exec", "review", "--json"].into_iter().map(str::to_string));
    push_optional_exec_review_settings(&mut args, settings);
    args.push("-c".to_string());
    args.push(developer_instructions);
    args.append(&mut parsed);
    Ok(args)
}

pub(super) fn codex_exec_args(
    thread_id: Option<&str>,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Vec<String> {
    let developer_instructions = codex_config_arg(
        CodexConfigKey::DeveloperInstructions,
        addness_tui_developer_instructions(),
    );
    let mut args = Vec::new();
    if let Some(thread_id) = thread_id {
        push_global_exec_settings(&mut args, settings, true);
        args.extend(["exec", "resume", "--json"].into_iter().map(str::to_string));
        push_optional_exec_settings(&mut args, settings);
        args.push("-c".to_string());
        args.push(developer_instructions);
        args.push(thread_id.to_string());
        args.push("-".to_string());
        return args;
    }

    push_global_exec_settings(&mut args, settings, false);
    args.extend(
        ["exec", "--json", "--color"]
            .into_iter()
            .map(str::to_string),
    );
    args.push(settings.color.label().to_string());
    if !settings.bypass_approvals_and_sandbox {
        args.push("-s".to_string());
        args.push(settings.sandbox.cli_arg().to_string());
    }
    args.push("-C".to_string());
    args.push(cwd.to_string());
    push_optional_exec_settings(&mut args, settings);
    args.push("-c".to_string());
    args.push(developer_instructions);
    args.push("-".to_string());
    args
}

fn toml_basic_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_addness_developer_instructions(args: &[String]) -> bool {
        args.iter()
            .any(|arg| arg.starts_with("developer_instructions="))
    }

    fn has_addness_memory_defaults(args: &[String]) -> bool {
        args.windows(2)
            .any(|pair| pair == ["-c", "memories.use_memories=false"])
            && args
                .windows(2)
                .any(|pair| pair == ["-c", "memories.generate_memories=false"])
    }

    #[test]
    fn codex_exec_args_start_new_json_turn() {
        let settings = CodexExecSettings::default();
        let args = codex_exec_args(None, "/repo", &settings);

        assert!(args.windows(2).any(|pair| pair == ["exec", "--json"]));
        assert!(args.contains(&"--json".to_string()));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-s", "workspace-write"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert_eq!(args.last().map(String::as_str), Some("-"));
        assert!(
            args.iter()
                .any(|arg| arg.starts_with("developer_instructions="))
        );
        assert!(args.iter().any(|arg| {
            arg.starts_with("developer_instructions=")
                && arg.contains("ADDNESS_WORKTREE_BRANCH")
                && arg.contains("Addness TUIは誰でも `addness` と打てば起動")
                && arg.contains("snapshotを最初の想起として扱ってください")
                && arg.contains("実装判断を変え得る場合だけ")
                && arg.contains("TUI snapshotを見る → リポジトリを読む → 実装/調査する → 検証する")
                && arg.contains("追加読込が必要な時")
                && arg.contains("実装判断に必要な不足分だけ")
                && arg.contains("turn完了・セッション終了サマリ")
                && arg.contains("手動でbody更新しなくて構いません")
                && arg.contains("手を止めてAddness更新ターンへ寄せない")
                && arg.contains("手動でAddnessに書き込むのは、自動メモでは足りない")
                && !arg.contains("Addnessに書き込むのは、作業を始めた時")
                && !arg.contains("読み取り時:")
        }));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-c", "memories.use_memories=false"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-c", "memories.generate_memories=false"])
        );
    }

    #[test]
    fn codex_exec_args_resume_existing_json_thread() {
        let settings = CodexExecSettings::default();
        let args = codex_exec_args(Some("thread-1"), "/repo", &settings);

        assert!(
            args.windows(3)
                .any(|triple| triple == ["exec", "resume", "--json"])
        );
        assert!(args.contains(&"--json".to_string()));
        assert!(args.contains(&"thread-1".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("-"));
        assert!(!args.contains(&"-C".to_string()));
        assert!(!args.contains(&"-s".to_string()));
    }

    #[test]
    fn codex_exec_resume_args_include_selected_settings() {
        let mut settings = CodexExecSettings::default();
        settings.model = CodexModelChoice::Gpt5;
        settings.reasoning = CodexReasoningChoice::Medium;
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        settings.image_paths.push("/tmp/shot.png".to_string());
        settings.output_schema = Some("/tmp/schema.json".to_string());
        settings.output_last_message = Some("/tmp/last.txt".to_string());

        let args = codex_exec_resume_args(None, true, false, "continue", &settings);

        assert!(args.windows(2).any(|pair| pair == ["-a", "on-request"]));
        assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]));
        assert_eq!(args.first().map(String::as_str), Some("-a"));
        assert_eq!(args.get(1).map(String::as_str), Some("on-request"));
        assert_eq!(args.get(2).map(String::as_str), Some("-s"));
        assert!(args.contains(&"--last".to_string()));
        assert!(args.contains(&"--json".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5"]));
        assert!(args.windows(2).any(|pair| pair == ["-i", "/tmp/shot.png"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--output-schema", "/tmp/schema.json"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-o", "/tmp/last.txt"]));
        assert!(
            args.iter()
                .any(|arg| arg.starts_with("developer_instructions="))
        );
        assert_eq!(args.last().map(String::as_str), Some("continue"));

        let session_args =
            codex_exec_resume_args(Some("session-1"), false, true, "next", &settings);
        assert!(session_args.contains(&"session-1".to_string()));
        assert!(!session_args.contains(&"--last".to_string()));
        assert!(session_args.contains(&"--all".to_string()));
        assert_eq!(session_args.last().map(String::as_str), Some("next"));
    }

    #[test]
    fn codex_root_interactive_args_include_prompt_and_settings() {
        let mut settings = CodexExecSettings::default();
        settings.model_override = Some("gpt-custom".to_string());
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        settings.web_search = true;

        let args = codex_root_interactive_args("hello codex", "/repo", &settings);

        assert!(args.windows(2).any(|pair| pair == ["-a", "on-request"]));
        assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]));
        assert!(args.contains(&"--search".to_string()));
        assert!(args.contains(&"--no-alt-screen".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-custom"]));
        assert!(
            args.iter()
                .any(|arg| arg.starts_with("developer_instructions="))
        );
        assert!(has_addness_memory_defaults(&args));
        assert_eq!(args.last().map(String::as_str), Some("hello codex"));

        let no_prompt = codex_root_interactive_args("", "/repo", &settings);
        assert!(has_addness_memory_defaults(&no_prompt));
        assert_ne!(no_prompt.last().map(String::as_str), Some(""));
    }

    #[test]
    fn codex_root_resume_args_include_interactive_resume_settings() {
        let mut settings = CodexExecSettings::default();
        settings.model = CodexModelChoice::Gpt5;
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        settings.web_search = true;

        let args = codex_root_resume_args(None, true, true, true, "continue", "/repo", &settings);

        assert!(args.windows(2).any(|pair| pair == ["-a", "on-request"]));
        assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]));
        assert!(args.contains(&"--search".to_string()));
        assert!(args.contains(&"--no-alt-screen".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5"]));
        assert!(has_addness_developer_instructions(&args));
        assert!(has_addness_memory_defaults(&args));
        assert!(args.contains(&"resume".to_string()));
        assert!(args.contains(&"--all".to_string()));
        assert!(args.contains(&"--include-non-interactive".to_string()));
        assert!(args.contains(&"--last".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("continue"));
        assert_eq!(codex_command_category(&args), "session");

        let session_args = codex_root_resume_args(
            Some("session-1"),
            false,
            false,
            false,
            "next",
            "/repo",
            &settings,
        );
        assert!(session_args.contains(&"session-1".to_string()));
        assert!(!session_args.contains(&"--last".to_string()));
        assert!(!session_args.contains(&"--include-non-interactive".to_string()));
        assert!(has_addness_developer_instructions(&session_args));
        assert!(has_addness_memory_defaults(&session_args));
        assert_eq!(session_args.last().map(String::as_str), Some("next"));
    }

    #[test]
    fn codex_root_session_command_args_pass_through_args_and_settings() {
        let mut settings = CodexExecSettings::default();
        settings.model_override = Some("gpt-custom".to_string());
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        settings.web_search = true;

        let args = codex_root_session_command_args(
            "fork",
            "--all 019f3042-1234-7000-8000-123456789abc \"try this\"",
            "/repo",
            &settings,
        )
        .unwrap();

        assert!(args.windows(2).any(|pair| pair == ["-a", "on-request"]));
        assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]));
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-custom"]));
        assert!(args.contains(&"--search".to_string()));
        assert!(args.contains(&"--no-alt-screen".to_string()));
        assert!(args.contains(&"fork".to_string()));
        assert!(has_addness_developer_instructions(&args));
        assert!(has_addness_memory_defaults(&args));
        assert!(args.contains(&"--all".to_string()));
        assert!(args.contains(&"019f3042-1234-7000-8000-123456789abc".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("try this"));
    }

    #[test]
    fn codex_fork_args_include_root_interactive_settings() {
        let mut settings = CodexExecSettings::default();
        settings.model = CodexModelChoice::Gpt5;
        settings.reasoning = CodexReasoningChoice::High;
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        settings.web_search = true;
        settings.image_paths.push("/tmp/shot.png".to_string());

        let args = codex_fork_args(Some("session-1"), false, true, "branch", "/repo", &settings);

        assert!(args.windows(2).any(|pair| pair == ["-a", "on-request"]));
        assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]));
        assert!(args.contains(&"--search".to_string()));
        assert!(args.contains(&"--no-alt-screen".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5"]));
        assert!(args.windows(2).any(|pair| pair == ["-i", "/tmp/shot.png"]));
        assert!(
            args.iter()
                .any(|arg| arg == "model_reasoning_effort=\"high\"")
        );
        assert!(has_addness_developer_instructions(&args));
        assert!(has_addness_memory_defaults(&args));
        assert!(args.contains(&"fork".to_string()));
        assert!(args.contains(&"--all".to_string()));
        assert!(args.contains(&"session-1".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("branch"));

        let last_args = codex_fork_args(None, true, false, "", "/repo", &settings);
        assert!(last_args.contains(&"--last".to_string()));
        assert!(!last_args.contains(&"--all".to_string()));
        assert!(has_addness_developer_instructions(&last_args));
        assert!(has_addness_memory_defaults(&last_args));
    }

    #[test]
    fn codex_session_admin_args_include_root_interactive_settings() {
        let mut settings = CodexExecSettings::default();
        settings.remote_addr = Some("ws://127.0.0.1:7777".to_string());
        settings.model = CodexModelChoice::Gpt5;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        settings
            .config_overrides
            .push("features.foo=true".to_string());

        let args = codex_session_admin_args(
            "delete",
            "019f3042-1234-7000-8000-123456789abc",
            true,
            vec!["--dry-run".to_string()],
            "/repo",
            &settings,
        );

        assert!(
            args.windows(2)
                .any(|pair| pair == ["--remote", "ws://127.0.0.1:7777"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]));
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-c", "features.foo=true"])
        );
        assert!(args.contains(&"delete".to_string()));
        assert!(args.contains(&"--force".to_string()));
        assert!(args.contains(&"019f3042-1234-7000-8000-123456789abc".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("--dry-run"));
        assert_eq!(codex_command_category(&args), "session");
    }

    #[test]
    fn codex_exec_review_args_include_exec_review_settings() {
        let mut settings = CodexExecSettings::default();
        settings.model = CodexModelChoice::Gpt5;
        settings.reasoning = CodexReasoningChoice::High;
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.strict_config = true;
        settings.ignore_rules = true;
        settings.output_schema = Some("/tmp/schema.json".to_string());

        let args = codex_exec_review_args("--uncommitted --title WIP", "/repo", &settings).unwrap();

        assert!(args.windows(2).any(|pair| pair == ["-a", "on-request"]));
        assert!(args.contains(&"--strict-config".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert!(
            args.windows(3)
                .any(|triple| triple == ["exec", "review", "--json"])
        );
        assert!(args.contains(&"--ignore-rules".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5"]));
        assert!(
            args.iter()
                .any(|arg| arg == "model_reasoning_effort=\"high\"")
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--output-schema", "/tmp/schema.json"])
        );
        assert!(
            args.iter()
                .any(|arg| arg.starts_with("developer_instructions="))
        );
        assert!(args.contains(&"--uncommitted".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["--title", "WIP"]));
        assert_eq!(codex_command_category(&args), "agent");
    }

    #[test]
    fn codex_review_args_include_root_review_settings() {
        let mut settings = CodexExecSettings::default();
        settings.remote_addr = Some("ws://127.0.0.1:7777".to_string());
        settings.model = CodexModelChoice::Gpt5;
        settings.strict_config = true;
        settings
            .config_overrides
            .push("features.foo=true".to_string());

        let args = codex_review_args("--base main --title Check", "/repo", &settings).unwrap();

        assert!(
            args.windows(2)
                .any(|pair| pair == ["--remote", "ws://127.0.0.1:7777"])
        );
        assert!(args.contains(&"--strict-config".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-c", "features.foo=true"])
        );
        assert!(args.contains(&"review".to_string()));
        assert!(
            args.iter()
                .any(|arg| arg.starts_with("developer_instructions="))
        );
        assert!(args.windows(2).any(|pair| pair == ["--base", "main"]));
        assert!(args.windows(2).any(|pair| pair == ["--title", "Check"]));
        assert_eq!(codex_command_category(&args), "workspace");
    }

    #[test]
    fn codex_apply_args_include_apply_settings() {
        let mut settings = CodexExecSettings::default();
        settings
            .config_overrides
            .push("features.foo=true".to_string());
        settings.enabled_features.push("responses_api".to_string());
        settings.disabled_features.push("legacy_mode".to_string());

        let args = codex_apply_args("task-1", &settings).unwrap();

        assert!(
            args.windows(2)
                .any(|pair| pair == ["-c", "features.foo=true"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--enable", "responses_api"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--disable", "legacy_mode"])
        );
        assert!(args.contains(&"apply".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("task-1"));
        assert_eq!(codex_command_category(&args), "workspace");

        let err = codex_apply_args("", &settings).unwrap_err();
        assert!(err.to_string().contains("task id"));
    }

    #[test]
    fn codex_exec_args_include_selected_exec_settings() {
        let mut settings = CodexExecSettings::default();
        settings.model = CodexModelChoice::Gpt5;
        settings.reasoning = CodexReasoningChoice::High;
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        let args = codex_exec_args(None, "/repo", &settings);

        assert!(
            args.windows(2)
                .next()
                .is_some_and(|pair| pair == ["-a", "on-request"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5"]));
        assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]));
        assert!(
            args.iter()
                .any(|arg| arg == "model_reasoning_effort=\"high\"")
        );
    }

    #[test]
    fn codex_exec_args_include_custom_model_override() {
        let mut settings = CodexExecSettings::default();
        settings.model_override = Some("gpt-custom".to_string());

        let args = codex_exec_args(None, "/repo", &settings);

        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-custom"]));
    }

    #[test]
    fn codex_exec_args_include_advanced_codex_cli_options() {
        let mut settings = CodexExecSettings::default();
        settings.web_search = true;
        settings.oss = true;
        settings.remote_addr = Some("ws://127.0.0.1:7777".to_string());
        settings.remote_auth_token_env = Some("CODEX_REMOTE_TOKEN".to_string());
        settings.no_alt_screen = true;
        settings.local_provider = CodexLocalProviderChoice::Ollama;
        settings.profile = Some("work".to_string());
        settings.additional_dirs.push("/tmp/extra".to_string());
        settings.image_paths.push("/tmp/shot.png".to_string());
        settings
            .config_overrides
            .push("features.foo=true".to_string());
        settings.enabled_features.push("responses_api".to_string());
        settings.disabled_features.push("legacy_mode".to_string());
        settings.strict_config = true;
        settings.ignore_user_config = true;
        settings.ignore_rules = true;
        settings.skip_git_repo_check = true;
        settings.ephemeral = true;
        settings.bypass_hook_trust = true;
        settings.color = CodexColorChoice::Always;
        settings.output_schema = Some("/tmp/schema.json".to_string());
        settings.output_last_message = Some("/tmp/last.txt".to_string());
        let args = codex_exec_args(None, "/repo", &settings);

        assert!(args.contains(&"--search".to_string()));
        assert!(args.contains(&"--oss".to_string()));
        assert!(args.contains(&"--strict-config".to_string()));
        assert!(args.contains(&"--dangerously-bypass-hook-trust".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["--color", "always"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--remote", "ws://127.0.0.1:7777"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--remote-auth-token-env", "CODEX_REMOTE_TOKEN"])
        );
        assert!(args.contains(&"--no-alt-screen".to_string()));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--local-provider", "ollama"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-p", "work"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--add-dir", "/tmp/extra"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-i", "/tmp/shot.png"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-c", "features.foo=true"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--enable", "responses_api"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--disable", "legacy_mode"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--output-schema", "/tmp/schema.json"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-o", "/tmp/last.txt"]));
        assert!(args.contains(&"--ignore-user-config".to_string()));
        assert!(args.contains(&"--ignore-rules".to_string()));
        assert!(args.contains(&"--skip-git-repo-check".to_string()));
        assert!(args.contains(&"--ephemeral".to_string()));
    }

    #[test]
    fn codex_exec_args_bypass_omits_approval_and_sandbox_flags() {
        let mut settings = CodexExecSettings::default();
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        settings.bypass_approvals_and_sandbox = true;

        let args = codex_exec_args(None, "/repo", &settings);

        assert!(args.contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()));
        assert!(!args.contains(&"-a".to_string()));
        assert!(!args.contains(&"-s".to_string()));
    }

    #[test]
    fn codex_named_subcommand_args_cover_common_codex_commands() {
        assert_eq!(
            codex_named_subcommand_args("doctor", "--help").unwrap(),
            vec!["doctor", "--help"]
        );
        assert_eq!(
            codex_named_subcommand_args("features", "").unwrap(),
            vec!["features", "list"]
        );
        assert_eq!(
            codex_named_subcommand_args("mcp", "").unwrap(),
            vec!["mcp", "list"]
        );
        assert_eq!(
            codex_named_subcommand_args("plugin", "").unwrap(),
            vec!["plugin", "list"]
        );
        assert_eq!(
            codex_named_subcommand_args("cloud", "").unwrap(),
            vec!["cloud", "list"]
        );
        assert_eq!(
            codex_named_subcommand_args("login", "").unwrap(),
            vec!["login", "status"]
        );
        assert_eq!(
            codex_named_subcommand_args("app-server", "").unwrap(),
            vec!["app-server", "daemon", "version"]
        );
        assert_eq!(
            codex_named_subcommand_args("mcp-server", "").unwrap(),
            vec!["mcp-server", "--help"]
        );
        assert_eq!(
            codex_named_subcommand_args("exec-server", "").unwrap(),
            vec!["exec-server", "--help"]
        );
        assert!(
            codex_named_subcommand_args("codex-help", "mcp")
                .err()
                .is_some()
        );
        assert_eq!(
            codex_named_subcommand_args("help", "mcp").unwrap(),
            vec!["help", "mcp"]
        );
        assert_eq!(
            codex_named_subcommand_args("version", "").unwrap(),
            vec!["--version"]
        );
        assert_eq!(
            codex_named_subcommand_args("review", "--uncommitted").unwrap(),
            vec!["review", "--uncommitted"]
        );
        assert_eq!(
            codex_named_subcommand_args("exec-review", "--uncommitted").unwrap(),
            vec!["exec", "review", "--json", "--uncommitted"]
        );
        assert_eq!(
            codex_named_subcommand_args("apply", "task-1").unwrap(),
            vec!["apply", "task-1"]
        );
    }

    #[test]
    fn codex_command_category_labels_management_commands() {
        assert_eq!(codex_command_category(&["login".to_string()]), "auth");
        assert_eq!(codex_command_category(&["cloud".to_string()]), "cloud");
        assert_eq!(
            codex_command_category(&["app-server".to_string()]),
            "server"
        );
        assert_eq!(codex_command_category(&["mcp".to_string()]), "config");
        assert_eq!(codex_command_category(&["delete".to_string()]), "session");
        assert_eq!(
            codex_command_category(&[
                "-a".to_string(),
                "on-request".to_string(),
                "--no-alt-screen".to_string(),
                "fork".to_string(),
            ]),
            "session"
        );
    }

    #[test]
    fn codex_named_subcommand_args_requires_apply_task_id() {
        let err = codex_named_subcommand_args("apply", "").unwrap_err();

        assert!(err.to_string().contains("task id"));
    }

    #[test]
    fn codex_named_subcommand_args_with_settings_prefixes_global_config() {
        let mut settings = CodexExecSettings::default();
        settings.remote_addr = Some("ws://127.0.0.1:7777".to_string());
        settings.strict_config = true;
        settings
            .config_overrides
            .push("features.foo=true".to_string());
        settings.enabled_features.push("responses_api".to_string());
        settings.disabled_features.push("legacy_mode".to_string());

        let args =
            codex_named_subcommand_args_with_settings(vec!["mcp".into(), "list".into()], &settings);

        assert!(
            args.windows(2)
                .any(|pair| pair == ["--remote", "ws://127.0.0.1:7777"])
        );
        assert!(args.contains(&"--strict-config".to_string()));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-c", "features.foo=true"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--enable", "responses_api"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--disable", "legacy_mode"])
        );
        assert_eq!(codex_command_category(&args), "config");

        let version_args =
            codex_named_subcommand_args_with_settings(vec!["--version".into()], &settings);
        assert_eq!(version_args, vec!["--version".to_string()]);
    }

    #[test]
    fn codex_arbitrary_agent_subcommands_keep_addness_db_contract() {
        let settings = CodexExecSettings::default();

        for raw in [
            vec!["exec".to_string(), "--json".to_string()],
            vec!["review".to_string(), "--uncommitted".to_string()],
            vec!["resume".to_string(), "--last".to_string()],
            vec!["fork".to_string(), "--last".to_string()],
        ] {
            let args = codex_named_subcommand_args_with_settings(raw, &settings);
            assert!(has_addness_memory_defaults(&args), "{args:?}");
            assert!(has_addness_developer_instructions(&args), "{args:?}");
        }

        let config_args =
            codex_named_subcommand_args_with_settings(vec!["mcp".into(), "list".into()], &settings);
        assert!(has_addness_memory_defaults(&config_args));
        assert!(!has_addness_developer_instructions(&config_args));

        let version_args =
            codex_named_subcommand_args_with_settings(vec!["--version".into()], &settings);
        assert_eq!(version_args, vec!["--version".to_string()]);
    }

    #[test]
    fn parse_codex_session_meta_line_extracts_picker_fields() {
        let session = parse_codex_session_meta_line(
            r#"{"timestamp":"2026-07-05T01:00:00Z","type":"session_meta","payload":{"id":"019f3042-1234-7000-8000-123456789abc","timestamp":"2026-07-05T00:59:00Z","cwd":"/repo","thread_name":"作業メモ"}}"#,
        )
        .unwrap();

        assert_eq!(session.id, "019f3042-1234-7000-8000-123456789abc");
        assert_eq!(session.title, "作業メモ");
        assert_eq!(session.updated_at, "2026-07-05T00:59:00Z");
        assert_eq!(session.cwd.as_deref(), Some("/repo"));
    }

    #[test]
    fn load_codex_session_candidates_merges_index_and_session_meta() {
        let root = std::env::temp_dir().join(format!(
            "addness-codex-index-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let sessions_dir = root.join("sessions").join("2026").join("07").join("05");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        std::fs::write(
            root.join("session_index.jsonl"),
            r#"{"id":"019f3042-1234-7000-8000-123456789abc","thread_name":"index title","updated_at":"2026-07-05T01:02:00Z"}"#,
        )
        .unwrap();
        std::fs::write(
            sessions_dir.join("rollout.jsonl"),
            r#"{"timestamp":"2026-07-05T01:00:00Z","type":"session_meta","payload":{"id":"019f3042-1234-7000-8000-123456789abc","timestamp":"2026-07-05T01:00:00Z","cwd":"/repo","thread_name":"meta title"}}"#,
        )
        .unwrap();

        let sessions = load_codex_session_candidates_from(&root, 10).unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "index title");
        assert_eq!(sessions[0].updated_at, "2026-07-05T01:02:00Z");
        assert_eq!(sessions[0].cwd.as_deref(), Some("/repo"));

        let _ = std::fs::remove_dir_all(root);
    }
}
