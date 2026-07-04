//! TUI 内で `codex exec --json` を起動し、JSONL イベントを Addness 側の
//! 会話履歴として描画するためのモジュール。
//!
//! Codex の対話型 TUI は使わず、入力・履歴・スクロール・イベント表示を
//! Addness 側で持つ。各ユーザー入力ごとに `codex exec --json` を 1 ターン実行し、
//! 2 ターン目以降は `codex exec resume <thread_id> --json` で同じ Codex セッションを
//! 継続する。

use std::collections::{BTreeSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use anyhow::{Context, Result};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

pub const CODEX_LOG_PREFIX_WIDTH: usize = 7;
const CODEX_SESSION_HISTORY_DIR: &str = "codex-sessions";
const CODEX_SESSION_HISTORY_MAX_LOG_LINES: usize = 5_000;
const CODEX_SESSION_HISTORY_MAX_RECORDS: usize = 20_000;
const CODEX_SESSION_HISTORY_MAX_BYTES: u64 = 20 * 1024 * 1024;

/// codex 実行ファイルのパスを解決する。
/// 環境変数 `ADDNESS_CODEX_BIN` を最優先で見て、無ければ PATH 上を探す。
/// 見つからなければ `None`。
pub fn codex_path() -> Option<PathBuf> {
    // 明示指定（別パスにインストールした場合や検証用の上書き）を最優先。
    if let Some(bin) = std::env::var_os("ADDNESS_CODEX_BIN") {
        let cand = PathBuf::from(bin);
        if cand.is_file() {
            return Some(cand);
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join("codex");
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

/// 作業ディレクトリの `git diff --stat`（HEAD 比較）を取得する。
/// 還流コメントのプリフィルに使う。取得できなければ空文字。
pub fn git_diff_stat(cwd: &Path) -> String {
    let output = std::process::Command::new("git")
        .args(["diff", "--stat", "HEAD"])
        .current_dir(cwd)
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

/// Addness 独自 UI に表示する Codex ログ行の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodexLogKind {
    User,
    Assistant,
    Tool,
    Turn,
    System,
    Error,
    Event,
}

/// Addness 独自 UI に表示する Codex ログ行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexLogLine {
    pub kind: CodexLogKind,
    pub text: String,
}

impl CodexLogLine {
    fn new(kind: CodexLogKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexLogFilter {
    All,
    Conversation,
    Tools,
    Errors,
}

impl CodexLogFilter {
    fn next(self) -> Self {
        match self {
            Self::All => Self::Conversation,
            Self::Conversation => Self::Tools,
            Self::Tools => Self::Errors,
            Self::Errors => Self::All,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Conversation => "Talk",
            Self::Tools => "Tools",
            Self::Errors => "Errors",
        }
    }
}

fn matches_log_filter(kind: CodexLogKind, filter: CodexLogFilter) -> bool {
    match filter {
        CodexLogFilter::All => true,
        CodexLogFilter::Conversation => {
            matches!(
                kind,
                CodexLogKind::User | CodexLogKind::Assistant | CodexLogKind::Turn
            )
        }
        CodexLogFilter::Tools => {
            matches!(kind, CodexLogKind::Tool | CodexLogKind::Event)
        }
        CodexLogFilter::Errors => matches!(kind, CodexLogKind::Error),
    }
}

enum CodexProcessEvent {
    Stdout(String),
    Stderr(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case")]
enum CodexSessionRecord {
    Log { kind: CodexLogKind, text: String },
    UpdateTurn { turn: usize, text: String },
    AssistantDelta { text: String },
    RawEvent { stream: String, line: String },
}

struct LoadedCodexSession {
    log: Vec<CodexLogLine>,
    record_count: usize,
}

struct CodexPaneSpawnOptions<'a> {
    codex_bin: &'a Path,
    cwd: &'a Path,
    addness_bin: &'a str,
    goal_id: String,
    goal_title: String,
    dod: String,
    status_label: String,
    session_log_path: Option<PathBuf>,
}

/// 子ゴール 1 件の表示用情報。
pub struct ChildGoal {
    pub id: String,
    pub title: String,
    pub icon: &'static str,
    /// 新着ハイライトの有効期限（None=通常表示）。
    pub new_until: Option<Instant>,
}

/// ホスト側 TUI が端末へ通知すべき Codex イベント。
pub struct TerminalNotice {
    pub title: String,
    pub message: String,
}

/// 初回ユーザー依頼から作る Codex タスク子ゴールの入力情報。
pub struct PendingCodexTaskGoal {
    pub parent_goal_id: String,
    pub parent_goal_title: String,
    pub prompt: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexRunState {
    InputWaiting,
    Thinking,
    CommandRunning,
    Confirming,
    Completed,
}

impl CodexRunState {
    pub fn label(self) -> &'static str {
        match self {
            Self::InputWaiting => "入力待ち",
            Self::Thinking => "考え中",
            Self::CommandRunning => "コマンド実行中",
            Self::Confirming => "確認待ち",
            Self::Completed => "完了",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::InputWaiting => "READY",
            Self::Thinking => "THINK",
            Self::CommandRunning => "RUN",
            Self::Confirming => "WAIT",
            Self::Completed => "DONE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexDecisionKind {
    Approval,
    Permission,
    Dangerous,
    YesNo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexDecisionBanner {
    pub kind: CodexDecisionKind,
    pub message: String,
    pub accept_key: char,
    pub accept_label: &'static str,
    pub deny_key: char,
    pub deny_label: &'static str,
}

impl CodexDecisionBanner {
    fn new(kind: CodexDecisionKind, message: String) -> Self {
        match kind {
            CodexDecisionKind::Approval => Self {
                kind,
                message,
                accept_key: 'a',
                accept_label: "承認",
                deny_key: 'd',
                deny_label: "拒否",
            },
            CodexDecisionKind::Permission | CodexDecisionKind::Dangerous => Self {
                kind,
                message,
                accept_key: 'a',
                accept_label: "許可",
                deny_key: 'd',
                deny_label: "拒否",
            },
            CodexDecisionKind::YesNo => Self {
                kind,
                message,
                accept_key: 'y',
                accept_label: "Yes",
                deny_key: 'n',
                deny_label: "No",
            },
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodexWorkSummary {
    pub implemented: Vec<String>,
    pub checks: Vec<String>,
    pub remaining: Vec<String>,
}

/// 埋め込み codex セッションの状態。
pub struct CodexPane {
    codex_bin: PathBuf,
    addness_bin: String,
    child: Option<Child>,
    tx: Sender<CodexProcessEvent>,
    rx: Receiver<CodexProcessEvent>,
    /// Codex プロセスが終了済みか。通常のターン完了では true にせず、
    /// ユーザーが `/exit` した場合やペインを閉じる場合にだけ終了扱いにする。
    pub finished: bool,
    /// 現在の `codex exec` ターンが実行中か。
    turn_running: bool,
    rows: u16,
    cols: u16,
    /// 還流先となる対象ゴールの ID。
    pub goal_id: String,
    /// codex 起動元として選択された親ゴール。初回依頼から子ゴールを作った後も保持する。
    pub parent_goal_id: String,
    pub parent_goal_title: String,
    /// 契約ペイン表示用に保持する対象ゴールのタイトルと DoD。
    pub goal_title: String,
    pub dod: String,
    /// codex が参照しているローカルの作業ディレクトリ（cwd）。
    pub cwd: String,
    /// 対象ゴールの現在ステータス表示（例: "進行中"）。
    pub status_label: String,
    /// DoD を行単位に分割した項目（契約ペインのチェックリスト用）。
    pub dod_items: Vec<String>,
    /// 各 DoD 項目の達成判定。None=未判定 / Some(true)=達成 / Some(false)=未達。
    pub dod_checks: Vec<Option<bool>>,
    /// DoD 自動判定（codex exec）を実行中か。
    pub assessing: bool,
    /// 子ゴール数・コメント数（変化検知で更新ログに反映する。未取得は None）。
    pub child_count: Option<usize>,
    pub comment_count: Option<usize>,
    pub deliverable_count: Option<usize>,
    /// PR / Release / tag など、作業の成果物トレースとして見せるリンク名。
    pub trace_links: Vec<String>,
    /// 子ゴールのライブリスト（新着は new_until までハイライト）。
    pub children: Vec<ChildGoal>,
    /// codex が直近に実行した addness 操作の表示ラベル（参照/書込中インジケータ）。
    pub action: Option<String>,
    /// codex が現在実行中として報告したコマンド。
    current_command: Option<String>,
    current_command_started_at: Option<Instant>,
    /// Addness をメモリとして使っているかを左ペインで示すための最終読込/書込時刻。
    pub last_addness_read_at: Option<Instant>,
    pub last_addness_write_at: Option<Instant>,
    pub last_addness_read_label: Option<String>,
    pub last_addness_write_label: Option<String>,
    /// ステータス・DoD が変化した時刻（変化行を数秒ハイライトするのに使う）。
    pub status_changed_at: Option<Instant>,
    pub dod_changed_at: Option<Instant>,
    /// codex ログのスクロールバック位置（0=最新、増えるほど過去）。
    pub scrollback: usize,
    /// 直近の描画で計算した表示行数ベースの最大スクロールバック。
    rendered_history_max_scrollback: Option<usize>,
    /// Addness 側の更新ログ（codex の書き込みやステータス変化を可視化）。新しいものほど末尾。
    pub activity: Vec<String>,
    /// codex セッション終了時の Addness body 自動記録を試行済みか。
    pub auto_record_attempted: bool,
    /// 最初の実依頼の body 自動記録を済み（再試行しない）とするフラグ。
    body_record_done: bool,
    input_state: CodexInputState,
    queued_prompts: VecDeque<String>,
    /// 初回実依頼から子ゴールを作るまで保留しているユーザー入力。
    pending_task_prompt: Option<String>,
    task_goal_creation_started: bool,
    /// `codex exec --json` が返した Codex thread id。2ターン目以降の resume に使う。
    thread_id: Option<String>,
    /// いま実行中のターンに対応するユーザー入力。turn.started の見出しに使う。
    current_turn_prompt: Option<String>,
    /// Codex の turn 番号。UI上の区切りに使う。
    turn_count: usize,
    collapsed_turns: BTreeSet<usize>,
    /// Addness 側で保持する会話・イベント履歴。
    log: Vec<CodexLogLine>,
    /// Addness 側で永続化する codex exec JSONL/UI 履歴。
    session_log_path: Option<PathBuf>,
    /// セッションログファイル内の概算レコード数（trim 判断とUI表示用）。
    session_record_count: usize,
    /// 起動時に復元した表示用ログ件数。
    loaded_history_count: usize,
    /// ストリーミング中の assistant 行。delta イベントが来た場合はこの行を伸ばす。
    streaming_assistant_index: Option<usize>,
    /// 端末通知待ちイベント。
    pending_notices: VecDeque<TerminalNotice>,
    /// 直近の exec 子プロセス終了イベントを JSON 側で受け取ったか。
    turn_finished_by_event: bool,
    /// Codex 履歴の表示フィルタ。
    log_filter: CodexLogFilter,
    /// Codex 履歴検索クエリ。
    search_query: String,
    /// 検索入力中か。
    search_editing: bool,
    pending_decision: Option<CodexDecisionBanner>,
}

impl CodexPane {
    /// codex exec JSON セッションを開始する。
    ///
    /// 起動時点では Codex プロセスを走らせず、Addness 側の入力欄で最初の依頼を待つ。
    /// 最初の Enter で `codex exec --json` を起動し、以降は `codex exec resume` を使う。
    pub fn spawn(
        codex_bin: &Path,
        cwd: &Path,
        addness_bin: &str,
        goal_id: String,
        goal_title: String,
        dod: String,
        status_label: String,
    ) -> Result<Self> {
        Self::spawn_inner(CodexPaneSpawnOptions {
            codex_bin,
            cwd,
            addness_bin,
            session_log_path: codex_session_log_path(&goal_id),
            goal_id,
            goal_title,
            dod,
            status_label,
        })
    }

    fn spawn_inner(options: CodexPaneSpawnOptions<'_>) -> Result<Self> {
        let CodexPaneSpawnOptions {
            codex_bin,
            cwd,
            addness_bin,
            goal_id,
            goal_title,
            dod,
            status_label,
            session_log_path,
        } = options;
        let dod_items = split_dod_items(&dod);
        let dod_checks = vec![None; dod_items.len()];
        let (tx, rx) = mpsc::channel::<CodexProcessEvent>();
        let loaded = session_log_path
            .as_deref()
            .map(load_codex_session)
            .transpose()
            .unwrap_or(None)
            .unwrap_or_else(|| LoadedCodexSession {
                log: Vec::new(),
                record_count: 0,
            });
        let loaded_history_count = loaded.log.len();
        let turn_count = loaded
            .log
            .iter()
            .filter(|line| line.kind == CodexLogKind::Turn)
            .count();
        let collapsed_turns = (1..turn_count).collect::<BTreeSet<_>>();

        let mut pane = Self {
            codex_bin: codex_bin.to_path_buf(),
            addness_bin: addness_bin.to_string(),
            child: None,
            tx,
            rx,
            finished: false,
            turn_running: false,
            rows: 24,
            cols: 80,
            parent_goal_id: goal_id.clone(),
            parent_goal_title: goal_title.clone(),
            goal_id,
            goal_title,
            cwd: cwd.display().to_string(),
            status_label,
            dod_items,
            dod_checks,
            assessing: false,
            child_count: None,
            comment_count: None,
            deliverable_count: None,
            trace_links: Vec::new(),
            children: Vec::new(),
            action: None,
            current_command: None,
            current_command_started_at: None,
            last_addness_read_at: None,
            last_addness_write_at: None,
            last_addness_read_label: None,
            last_addness_write_label: None,
            status_changed_at: None,
            dod_changed_at: None,
            scrollback: 0,
            rendered_history_max_scrollback: None,
            activity: Vec::new(),
            auto_record_attempted: false,
            body_record_done: false,
            input_state: CodexInputState::default(),
            queued_prompts: VecDeque::new(),
            pending_task_prompt: None,
            task_goal_creation_started: false,
            thread_id: None,
            current_turn_prompt: None,
            turn_count,
            collapsed_turns,
            log: loaded.log,
            session_log_path,
            session_record_count: loaded.record_count,
            loaded_history_count,
            streaming_assistant_index: None,
            pending_notices: VecDeque::new(),
            turn_finished_by_event: false,
            log_filter: CodexLogFilter::All,
            search_query: String::new(),
            search_editing: false,
            pending_decision: None,
            dod,
        };
        if pane.loaded_history_count > 0 {
            pane.push_log(
                CodexLogKind::System,
                format!(
                    "前回のAddness UI履歴を {} 件読み込みました。続きは Enter で送信できます。",
                    pane.loaded_history_count
                ),
            );
        }
        pane.push_log(
            CodexLogKind::System,
            "Addness独自UIで待機中。入力して Enter で codex exec --json を実行します。",
        );
        Ok(pane)
    }

    #[cfg(test)]
    pub(crate) fn test_with_output(
        rows: u16,
        cols: u16,
        _scrollback_len: usize,
        output: &str,
    ) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut pane = Self::spawn(
            Path::new("codex"),
            &cwd,
            "addness",
            "test-goal".to_string(),
            "Test goal".to_string(),
            String::new(),
            "TEST".to_string(),
        )
        .unwrap();
        pane.rows = rows.max(1);
        pane.cols = cols.max(1);
        pane.log.clear();
        pane.session_log_path = None;
        pane.session_record_count = 0;
        pane.loaded_history_count = 0;
        pane.turn_count = 0;
        pane.rendered_history_max_scrollback = None;
        pane.log_filter = CodexLogFilter::All;
        pane.search_query.clear();
        pane.search_editing = false;
        pane.collapsed_turns.clear();
        pane.pending_decision = None;
        for line in output.lines() {
            pane.push_log(CodexLogKind::Assistant, line.to_string());
        }
        pane.finished = true;
        pane
    }

    /// ログを delta 行スクロールする（正=過去へ、負=最新へ）。
    pub fn scroll_lines(&mut self, delta: isize) {
        let max = self.max_view_scrollback();
        let target = (self.scrollback as isize + delta).max(0) as usize;
        self.scrollback = target.min(max);
    }

    /// 1 ページ分の行数（スクロール量）。
    pub fn page(&self) -> usize {
        self.rows.saturating_sub(3).max(1) as usize
    }

    /// 最古（バッファ先頭）までスクロールする。
    pub fn scroll_to_top(&mut self) {
        self.scrollback = self.max_view_scrollback();
    }

    /// 最新（ライブ）位置へ戻す。
    pub fn scroll_to_live(&mut self) {
        self.scrollback = 0;
    }

    pub fn is_turn_running(&self) -> bool {
        self.turn_running || self.child.is_some()
    }

    pub fn run_state(&self) -> CodexRunState {
        if self.finished {
            CodexRunState::Completed
        } else if self.pending_decision.is_some() {
            CodexRunState::Confirming
        } else if self.current_command.is_some() {
            CodexRunState::CommandRunning
        } else if self.task_goal_creation_started || self.is_turn_running() {
            CodexRunState::Thinking
        } else {
            CodexRunState::InputWaiting
        }
    }

    pub fn current_command(&self) -> Option<&str> {
        self.current_command.as_deref()
    }

    pub fn current_command_elapsed_secs(&self) -> Option<u64> {
        self.current_command_started_at
            .map(|t| t.elapsed().as_secs())
    }

    pub fn thread_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }

    pub fn turn_count(&self) -> usize {
        self.turn_count
    }

    pub fn collapsed_turn_count(&self) -> usize {
        self.collapsed_turns.len()
    }

    pub fn decision_banner(&self) -> Option<&CodexDecisionBanner> {
        self.pending_decision.as_ref()
    }

    pub fn last_assistant_text(&self) -> Option<&str> {
        self.log
            .iter()
            .rev()
            .find(|line| line.kind == CodexLogKind::Assistant)
            .map(|line| line.text.as_str())
    }

    pub fn input_line(&self) -> &str {
        &self.input_state.line
    }

    pub fn queued_prompt_count(&self) -> usize {
        self.queued_prompts.len()
    }

    pub fn filtered_log_lines(&self) -> Vec<&CodexLogLine> {
        let query = self.normalized_search_query();
        let collapse_turns = self.turns_are_collapsible_in_current_view();
        let mut current_collapsed_turn = None;
        let mut visible = Vec::new();
        for line in &self.log {
            if line.kind == CodexLogKind::Turn {
                current_collapsed_turn = turn_number_from_label(&line.text)
                    .filter(|n| collapse_turns && self.collapsed_turns.contains(n));
                if self.log_line_visible(line, &query) {
                    visible.push(line);
                }
                continue;
            }
            if current_collapsed_turn.is_some() {
                continue;
            }
            if self.log_line_visible(line, &query) {
                visible.push(line);
            }
        }
        visible
    }

    pub fn toggle_visible_turn_collapsed(&mut self) -> bool {
        if !self.turns_are_collapsible_in_current_view() {
            return false;
        }
        let target = if self.scrollback == 0 {
            self.latest_completed_turn()
        } else {
            self.turn_at_viewport_start()
                .filter(|turn| self.can_toggle_turn(*turn))
                .or_else(|| self.latest_completed_turn())
        };
        let Some(turn) = target else {
            return false;
        };
        self.toggle_turn_collapsed_by_number(turn)
    }

    pub fn toggle_turn_collapsed_by_number(&mut self, turn: usize) -> bool {
        if !self.turns_are_collapsible_in_current_view() {
            return false;
        }
        if !self.can_toggle_turn(turn) {
            return false;
        }
        self.toggle_turn_collapsed(turn);
        self.scroll_to_turn_header(turn);
        true
    }

    pub fn toggle_old_turns_collapsed(&mut self) {
        if self.collapsed_turns.is_empty() {
            self.collapse_completed_turns();
        } else {
            self.collapsed_turns.clear();
        }
        self.invalidate_rendered_history_metrics();
        self.scrollback = self.scrollback.min(self.max_view_scrollback());
    }

    fn toggle_turn_collapsed(&mut self, turn: usize) {
        if !self.collapsed_turns.remove(&turn) {
            self.collapsed_turns.insert(turn);
        }
        self.invalidate_rendered_history_metrics();
        self.scrollback = self.scrollback.min(self.max_view_scrollback());
    }

    fn collapse_completed_turns(&mut self) {
        self.collapsed_turns.clear();
        let keep_open = if self.turn_running {
            self.turn_count
        } else {
            self.turn_count.saturating_add(1)
        };
        for turn in 1..keep_open {
            self.collapsed_turns.insert(turn);
        }
    }

    fn turns_are_collapsible_in_current_view(&self) -> bool {
        self.search_query.trim().is_empty()
            && matches!(
                self.log_filter,
                CodexLogFilter::All | CodexLogFilter::Conversation
            )
    }

    fn turn_at_viewport_start(&self) -> Option<usize> {
        let lines = self.filtered_log_lines();
        let total_lines = lines
            .iter()
            .map(|line| rendered_log_line_height(&line.text, self.cols as usize))
            .sum::<usize>();
        let max_scrollback = self
            .rendered_history_max_scrollback
            .unwrap_or_else(|| total_lines.saturating_sub(self.log_viewport_height()));
        let viewport_top = max_scrollback.saturating_sub(self.scrollback);
        let mut active_turn = None;
        let mut line_top = 0usize;
        let mut viewport_reached = false;

        for line in lines {
            if line.kind == CodexLogKind::Turn {
                active_turn = turn_number_from_label(&line.text);
                if viewport_reached {
                    return active_turn;
                }
            }
            let line_bottom =
                line_top.saturating_add(rendered_log_line_height(&line.text, self.cols as usize));
            if viewport_top < line_bottom {
                if active_turn.is_some() {
                    return active_turn;
                }
                viewport_reached = true;
            }
            line_top = line_bottom;
        }

        active_turn
    }

    fn scroll_to_turn_header(&mut self, turn: usize) {
        let mut line_top = 0usize;
        for line in self.filtered_log_lines() {
            if line.kind == CodexLogKind::Turn && turn_number_from_label(&line.text) == Some(turn) {
                let max = self.max_view_scrollback();
                self.scrollback = max.saturating_sub(line_top);
                return;
            }
            line_top =
                line_top.saturating_add(rendered_log_line_height(&line.text, self.cols as usize));
        }
        self.scrollback = self.scrollback.min(self.max_view_scrollback());
    }

    fn latest_completed_turn(&self) -> Option<usize> {
        if self.turn_count == 0 {
            return None;
        }
        if self.turn_running {
            return self.turn_count.checked_sub(1).filter(|turn| *turn > 0);
        }
        Some(self.turn_count)
    }

    fn can_toggle_turn(&self, turn: usize) -> bool {
        turn > 0 && turn <= self.turn_count && !(self.turn_running && turn == self.turn_count)
    }

    pub fn work_summary(&self) -> CodexWorkSummary {
        CodexWorkSummary {
            implemented: summarize_implemented_work(&self.log),
            checks: summarize_checks(&self.log),
            remaining: summarize_remaining_work(&self.log),
        }
    }

    pub fn handle_decision_key(&mut self, key: KeyEvent) -> bool {
        let Some(decision) = self.pending_decision.clone() else {
            return false;
        };
        if !key.modifiers.is_empty() {
            return false;
        }
        let KeyCode::Char(ch) = key.code else {
            return false;
        };
        let ch = ch.to_ascii_lowercase();
        let response = if ch == decision.accept_key || ch == 'y' {
            Some(decision.accept_label)
        } else if ch == decision.deny_key || ch == 'n' {
            Some(decision.deny_label)
        } else {
            None
        };
        let Some(response) = response else {
            return false;
        };
        self.pending_decision = None;
        self.action = Some(format!("確認応答: {response}"));
        self.push_activity(format!("確認待ちに {response} で応答"));
        self.push_terminal_notice("Codex 確認応答", format!("{response} を選択しました"));
        true
    }

    pub fn log_filter_label(&self) -> &'static str {
        self.log_filter.label()
    }

    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    pub fn is_search_editing(&self) -> bool {
        self.search_editing
    }

    pub fn cycle_log_filter(&mut self) {
        self.log_filter = self.log_filter.next();
        self.invalidate_rendered_history_metrics();
        self.scrollback = self.scrollback.min(self.max_view_scrollback());
    }

    pub fn begin_search(&mut self) {
        self.search_editing = true;
        self.invalidate_rendered_history_metrics();
        self.scrollback = self.scrollback.min(self.max_view_scrollback());
    }

    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_editing = false;
        self.invalidate_rendered_history_metrics();
        self.scrollback = self.scrollback.min(self.max_view_scrollback());
    }

    pub fn handle_search_key(&mut self, key: KeyEvent) -> bool {
        if !self.search_editing {
            return false;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.search_editing = false;
                true
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.invalidate_rendered_history_metrics();
                self.scrollback = self.scrollback.min(self.max_view_scrollback());
                true
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_query.clear();
                self.invalidate_rendered_history_metrics();
                self.scrollback = self.scrollback.min(self.max_view_scrollback());
                true
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.search_query.push(ch);
                self.invalidate_rendered_history_metrics();
                self.scrollback = self.scrollback.min(self.max_view_scrollback());
                true
            }
            _ => true,
        }
    }

    pub fn history_label(&self) -> String {
        if self.session_log_path.is_some() {
            format!("履歴 {}件 保存中", self.log.len())
        } else {
            format!("履歴 {}件 メモリのみ", self.log.len())
        }
    }

    pub fn history_path_label(&self) -> Option<String> {
        self.session_log_path.as_deref().map(compact_home_path)
    }

    pub fn loaded_history_count(&self) -> usize {
        self.loaded_history_count
    }

    pub fn sync_rendered_history_metrics(&mut self, total_lines: usize, viewport_height: usize) {
        let max = total_lines.saturating_sub(viewport_height);
        self.rendered_history_max_scrollback = Some(max);
        self.scrollback = self.scrollback.min(max);
    }

    fn invalidate_rendered_history_metrics(&mut self) {
        self.rendered_history_max_scrollback = None;
    }

    fn max_view_scrollback(&self) -> usize {
        if let Some(max) = self.rendered_history_max_scrollback {
            return max;
        }
        self.filtered_log_lines()
            .iter()
            .map(|line| rendered_log_line_height(&line.text, self.cols as usize))
            .sum::<usize>()
            .saturating_sub(self.log_viewport_height())
    }

    fn normalized_search_query(&self) -> String {
        self.search_query.trim().to_lowercase()
    }

    fn log_line_visible(&self, line: &CodexLogLine, query: &str) -> bool {
        if !matches_log_filter(line.kind, self.log_filter) {
            return false;
        }
        if query.is_empty() {
            return true;
        }
        contains_case_insensitive(&line.text, query)
    }

    fn log_viewport_height(&self) -> usize {
        // 枠の上下と入力欄2行を除いた概算。描画側も同じ領域に収める。
        self.rows.saturating_sub(4).max(1) as usize
    }

    /// Addness 側の更新ログへ 1 行追加する（古いものから捨てて最大 50 件保持）。
    pub fn push_activity(&mut self, line: String) {
        self.activity.push(line);
        let len = self.activity.len();
        if len > 50 {
            self.activity.drain(0..len - 50);
        }
    }

    fn push_log(&mut self, kind: CodexLogKind, text: impl Into<String>) {
        self.invalidate_rendered_history_metrics();
        let line = CodexLogLine::new(kind, text);
        self.persist_session_record(CodexSessionRecord::Log {
            kind: line.kind,
            text: line.text.clone(),
        });
        self.log.push(line);
        self.trim_in_memory_log();
        if !matches!(kind, CodexLogKind::Assistant) {
            self.streaming_assistant_index = None;
        }
        if self.scrollback == 0 {
            self.scroll_to_live();
        } else {
            self.scrollback = self.scrollback.min(self.max_view_scrollback());
        }
    }

    fn append_assistant_delta(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.invalidate_rendered_history_metrics();
        if let Some(index) = self.streaming_assistant_index
            && let Some(line) = self.log.get_mut(index)
        {
            line.text.push_str(text);
            self.persist_session_record(CodexSessionRecord::AssistantDelta {
                text: text.to_string(),
            });
            return;
        }
        self.persist_session_record(CodexSessionRecord::Log {
            kind: CodexLogKind::Assistant,
            text: text.to_string(),
        });
        self.log
            .push(CodexLogLine::new(CodexLogKind::Assistant, text));
        self.trim_in_memory_log();
        self.streaming_assistant_index = Some(self.log.len().saturating_sub(1));
    }

    fn refresh_current_turn_title(&mut self) {
        let turn = self.turn_count;
        let Some(title) = summarize_turn_title(&self.log, turn) else {
            return;
        };
        let label = format!("Turn {turn} - {title}");
        if update_turn_log_line(&mut self.log, turn, label.clone()) {
            self.invalidate_rendered_history_metrics();
            self.persist_session_record(CodexSessionRecord::UpdateTurn { turn, text: label });
        }
    }

    fn trim_in_memory_log(&mut self) {
        let len = self.log.len();
        if len <= CODEX_SESSION_HISTORY_MAX_LOG_LINES {
            return;
        }
        let removed = len - CODEX_SESSION_HISTORY_MAX_LOG_LINES;
        self.log.drain(0..removed);
        self.streaming_assistant_index = self
            .streaming_assistant_index
            .and_then(|idx| idx.checked_sub(removed));
    }

    fn persist_raw_event(&mut self, stream: &str, line: &str) {
        self.persist_session_record(CodexSessionRecord::RawEvent {
            stream: stream.to_string(),
            line: line.to_string(),
        });
    }

    fn persist_session_record(&mut self, record: CodexSessionRecord) {
        let Some(path) = self.session_log_path.clone() else {
            return;
        };
        if append_codex_session_record(&path, &record).is_err() {
            return;
        }
        self.session_record_count = self.session_record_count.saturating_add(1);
        let should_trim = self.session_record_count > CODEX_SESSION_HISTORY_MAX_RECORDS
            || fs::metadata(&path)
                .map(|m| m.len() > CODEX_SESSION_HISTORY_MAX_BYTES)
                .unwrap_or(false);
        if should_trim && let Ok(count) = trim_codex_session_log(&path) {
            self.session_record_count = count;
        }
    }

    /// 子ゴールのライブリストを差し替える。新規 ID は一定時間ハイライトする。
    /// 初回（既存が空）の取得では全件を新着扱いしない。
    pub fn update_children(&mut self, incoming: Vec<(String, String, &'static str)>) {
        let had_any = !self.children.is_empty();
        let old_ids: std::collections::HashSet<String> =
            self.children.iter().map(|c| c.id.clone()).collect();
        let new_until = Instant::now() + std::time::Duration::from_secs(4);
        self.children = incoming
            .into_iter()
            .map(|(id, title, icon)| {
                let is_new = had_any && !old_ids.contains(&id);
                ChildGoal {
                    new_until: is_new.then_some(new_until),
                    id,
                    title,
                    icon,
                }
            })
            .collect();
    }

    /// JSONL と子プロセス終了を取り込み、画面に影響する変化があれば `true` を返す。
    pub fn update(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.rx.try_recv() {
            changed = true;
            match event {
                CodexProcessEvent::Stdout(line) => self.handle_stdout_line(&line),
                CodexProcessEvent::Stderr(line) => self.handle_stderr_line(&line),
            }
        }

        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.child = None;
                    self.turn_running = false;
                    self.current_command = None;
                    self.current_command_started_at = None;
                    self.streaming_assistant_index = None;
                    self.pending_decision = None;
                    if !self.turn_finished_by_event {
                        if status.success() {
                            self.refresh_current_turn_title();
                            self.push_log(CodexLogKind::System, "Codex ターンが完了しました");
                            self.push_terminal_notice("Codex 完了", "Codex の出力が完了しました");
                        } else {
                            let message = format!("Codex ターンが失敗しました: {status}");
                            self.push_log(CodexLogKind::Error, message.clone());
                            self.refresh_current_turn_title();
                            self.push_terminal_notice("Codex 失敗", message);
                        }
                    }
                    self.turn_finished_by_event = false;
                    self.current_turn_prompt = None;
                    self.start_next_queued_turn_if_idle();
                    changed = true;
                }
                Ok(None) => {}
                Err(e) => {
                    self.child = None;
                    self.turn_running = false;
                    self.current_command = None;
                    self.current_command_started_at = None;
                    self.pending_decision = None;
                    self.current_turn_prompt = None;
                    let message = format!("Codex 状態確認に失敗: {e}");
                    self.push_log(CodexLogKind::Error, message.clone());
                    self.push_terminal_notice("Codex エラー", message);
                    self.start_next_queued_turn_if_idle();
                    changed = true;
                }
            }
        }

        changed
    }

    fn handle_stdout_line(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        self.persist_raw_event("stdout", trimmed);
        match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => self.handle_json_event(value),
            Err(_) => self.push_log(CodexLogKind::Event, trimmed.to_string()),
        }
    }

    fn handle_stderr_line(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        // Codex の PATH alias 警告など、会話上重要でない既知ノイズは表示しない。
        if trimmed.contains("could not create PATH aliases") {
            return;
        }
        self.persist_raw_event("stderr", trimmed);
        if let Some(decision) = decision_banner("stderr", Some(trimmed)) {
            self.set_pending_decision(decision);
        }
        self.push_log(CodexLogKind::Event, trimmed.to_string());
    }

    fn handle_json_event(&mut self, value: Value) {
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("event")
            .to_string();

        match event_type.as_str() {
            "thread.started" => {
                if let Some(thread_id) = string_at_any(&value, &["thread_id", "threadId", "id"]) {
                    self.thread_id = Some(thread_id.clone());
                    self.push_log(CodexLogKind::System, format!("Codex thread: {thread_id}"));
                } else {
                    self.push_log(CodexLogKind::System, "Codex thread started");
                }
            }
            "turn.started" => {
                self.turn_running = true;
                self.turn_finished_by_event = false;
                self.current_command = None;
                self.current_command_started_at = None;
                self.pending_decision = None;
                if self.turn_count > 0 {
                    self.collapsed_turns.insert(self.turn_count);
                }
                self.turn_count = self.turn_count.saturating_add(1);
                let label = self
                    .current_turn_prompt
                    .as_deref()
                    .or_else(|| self.last_prompt())
                    .map(compact_turn_prompt)
                    .filter(|prompt| !prompt.is_empty())
                    .map(|prompt| format!("Turn {} - {prompt}", self.turn_count))
                    .unwrap_or_else(|| format!("Turn {}", self.turn_count));
                self.push_log(CodexLogKind::Turn, label);
            }
            "turn.completed" | "turn.finished" => {
                self.turn_running = false;
                self.turn_finished_by_event = true;
                self.current_command = None;
                self.current_command_started_at = None;
                self.streaming_assistant_index = None;
                self.pending_decision = None;
                self.refresh_current_turn_title();
                self.push_log(CodexLogKind::System, "Codex ターンが完了しました");
                self.push_terminal_notice("Codex 完了", "Codex の出力が完了しました");
            }
            "turn.failed" => {
                self.turn_running = false;
                self.turn_finished_by_event = true;
                self.current_command = None;
                self.current_command_started_at = None;
                self.streaming_assistant_index = None;
                self.pending_decision = None;
                let message = nested_error_message(&value)
                    .or_else(|| first_text_field(&value))
                    .unwrap_or_else(|| "Codex ターンが失敗しました".to_string());
                self.push_log(CodexLogKind::Error, message.clone());
                self.refresh_current_turn_title();
                self.push_terminal_notice("Codex 失敗", message);
            }
            "error" => {
                let message = nested_error_message(&value)
                    .or_else(|| first_text_field(&value))
                    .unwrap_or_else(|| "Codex エラー".to_string());
                self.push_log(CodexLogKind::Error, message.clone());
                self.push_terminal_notice("Codex エラー", message);
            }
            _ => self.handle_generic_json_event(&event_type, &value),
        }
    }

    fn handle_generic_json_event(&mut self, event_type: &str, value: &Value) {
        if let Some(decision) = decision_banner(event_type, first_text_field(value).as_deref()) {
            self.set_pending_decision(decision);
        }

        if let Some(display) = tool_display(event_type, value) {
            if let Some(command) = display.command_text.as_deref() {
                if is_tool_completion_event(event_type) {
                    self.current_command = None;
                    self.current_command_started_at = None;
                } else {
                    self.current_command = Some(compact_tool_text(command));
                    self.current_command_started_at = Some(Instant::now());
                }
            }
            if let Some(action_text) = display.action_text.as_deref() {
                self.refresh_action_from_text(action_text);
            }
            self.record_addness_tool_activity(&display);
            self.push_log(CodexLogKind::Tool, display.label);
            return;
        }

        if event_type.contains("message") || event_type.contains("output_text") {
            let text = first_text_field(value).unwrap_or_default();
            if text.is_empty() {
                self.push_log(CodexLogKind::Event, event_type.to_string());
            } else if event_type.contains("delta") {
                self.append_assistant_delta(&text);
            } else {
                self.streaming_assistant_index = None;
                self.push_log(CodexLogKind::Assistant, text);
            }
            return;
        }

        if event_type.contains("exec")
            || event_type.contains("tool")
            || event_type.contains("function")
            || event_type.contains("mcp")
        {
            let text = first_text_field(value).unwrap_or_else(|| event_type.to_string());
            if is_tool_completion_event(event_type) {
                self.current_command = None;
                self.current_command_started_at = None;
            } else {
                self.current_command = Some(compact_tool_text(&text));
                self.current_command_started_at = Some(Instant::now());
            }
            self.refresh_action_from_text(&text);
            self.push_log(CodexLogKind::Tool, format!("{event_type}: {text}"));
            return;
        }

        if let Some(text) = first_text_field(value)
            && !text.is_empty()
        {
            self.push_log(CodexLogKind::Event, format!("{event_type}: {text}"));
            return;
        }

        self.push_log(CodexLogKind::Event, event_type.to_string());
    }

    fn refresh_action_from_text(&mut self, text: &str) {
        if let Some(rest) = addness_command_rest(text) {
            let (label, kind) = action_label(rest);
            match kind {
                AddnessActionKind::Read => {
                    self.last_addness_read_at = Some(Instant::now());
                    self.last_addness_read_label = Some(label.clone());
                }
                AddnessActionKind::Write => {
                    self.last_addness_write_at = Some(Instant::now());
                    self.last_addness_write_label = Some(label.clone());
                }
            }
            self.action = Some(label);
        }
    }

    fn set_pending_decision(&mut self, decision: CodexDecisionBanner) {
        let message = decision.message.clone();
        self.pending_decision = Some(decision);
        self.push_terminal_notice("Codex 確認待ち", message);
    }

    fn record_addness_tool_activity(&mut self, display: &ToolDisplay) {
        let Some(command) = display.command_text.as_deref() else {
            return;
        };
        if addness_command_rest(command).is_none() && !looks_like_addness_command_text(command) {
            return;
        }
        let Some(activity) = addness_activity_summary(command, display.output_text.as_deref())
        else {
            return;
        };
        let now = chrono::Local::now().format("%H:%M");
        self.push_activity(format!("{now} {activity}"));
    }

    /// Codex の画面状態から、ホスト端末へ流すべき通知を 1 件だけ取り出す。
    pub fn take_terminal_notice(&mut self) -> Option<TerminalNotice> {
        self.pending_notices.pop_front()
    }

    fn push_terminal_notice(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.pending_notices.push_back(TerminalNotice {
            title: title.into(),
            message: message.into(),
        });
    }

    /// 描画領域に合わせて保持サイズを更新する（変化時のみ）。
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        self.invalidate_rendered_history_metrics();
        self.scrollback = self.scrollback.min(self.max_view_scrollback());
    }

    /// キー入力を Addness 側の入力欄へ反映する。
    pub fn input(&mut self, key: KeyEvent) {
        if self.finished {
            return;
        }

        if self.is_turn_running()
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'C'))
        {
            self.kill_current_turn();
            self.start_next_queued_turn_if_idle();
            return;
        }

        if self.pending_decision.is_some() {
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c' | 'C'))
            {
                self.kill_current_turn();
            }
            return;
        }

        let Some(submitted) = self.input_state.observe_key(key) else {
            return;
        };
        self.submit_user_line(&submitted);
    }

    /// TUI 側の操作からシステムプロンプト（F9 再開など）を codex に送信する。
    /// ユーザーの作業依頼ではないので last_prompt は更新しない。
    pub fn submit_system_line(&mut self, line: &str) {
        let submitted = normalize_submitted_line(line);
        if submitted.is_empty() || self.is_turn_running() || self.finished {
            return;
        }
        self.push_log(CodexLogKind::System, format!("Addness: {submitted}"));
        self.start_turn(&submitted);
    }

    fn submit_user_line(&mut self, submitted: &str) {
        let submitted = normalize_submitted_line(submitted);
        if submitted.is_empty() {
            return;
        }

        if self.pending_task_prompt.is_some() {
            self.queued_prompts.push_back(submitted);
            let count = self.queued_prompts.len();
            self.action = Some(format!("次ターン予約 {count}件"));
            self.push_log(
                CodexLogKind::System,
                format!("子ゴール作成中のため次のターンに予約しました（待ち{count}件）"),
            );
            return;
        }

        if self.is_turn_running() {
            self.record_and_run_user_line(submitted);
            return;
        }

        if self.should_create_task_goal_for_first_prompt(&submitted) {
            self.pending_task_prompt = Some(submitted);
            self.action = Some("子ゴール作成中".to_string());
            self.push_log(
                CodexLogKind::System,
                "最初の依頼をまとめる子ゴールを作成しています",
            );
            return;
        }

        self.record_and_run_user_line(submitted);
    }

    fn record_and_run_user_line(&mut self, submitted: String) {
        self.input_state.record_submitted(&submitted);
        self.push_log(CodexLogKind::User, submitted.clone());

        if self.is_turn_running() {
            self.queued_prompts.push_back(submitted);
            let count = self.queued_prompts.len();
            self.action = Some(format!("次ターン予約 {count}件"));
            self.push_log(
                CodexLogKind::System,
                format!("Codex 実行中のため次のターンに予約しました（待ち{count}件）"),
            );
            return;
        }

        self.run_submitted_line(submitted);
    }

    fn should_create_task_goal_for_first_prompt(&self, submitted: &str) -> bool {
        self.thread_id.is_none()
            && self.input_state.last_prompt.is_none()
            && !self.task_goal_creation_started
            && self.goal_id == self.parent_goal_id
            && submitted != "/exit"
    }

    fn run_submitted_line(&mut self, submitted: String) {
        if submitted == "/exit" {
            self.finish_from_exit_command();
            return;
        }

        self.start_turn(&submitted);
    }

    fn finish_from_exit_command(&mut self) {
        self.finished = true;
        self.queued_prompts.clear();
        self.pending_decision = None;
        self.current_command = None;
        self.current_command_started_at = None;
        self.current_turn_prompt = None;
        self.push_log(CodexLogKind::System, "Codex セッションを終了します");
    }

    fn start_turn(&mut self, prompt: &str) {
        if self.is_turn_running() {
            self.push_log(CodexLogKind::System, "前の Codex ターンがまだ実行中です");
            return;
        }

        let command_result = self.spawn_exec_process(prompt);
        match command_result {
            Ok(child) => {
                self.child = Some(child);
                self.turn_running = true;
                self.turn_finished_by_event = false;
                self.pending_decision = None;
                self.current_turn_prompt = Some(prompt.to_string());
                self.scroll_to_live();
            }
            Err(e) => {
                let message = format!("codex exec の起動に失敗しました: {e}");
                self.push_log(CodexLogKind::Error, message.clone());
                self.push_terminal_notice("Codex 起動失敗", message);
            }
        }
    }

    fn start_next_queued_turn_if_idle(&mut self) -> bool {
        if self.finished || self.is_turn_running() {
            return false;
        }
        let Some(submitted) = self.queued_prompts.pop_front() else {
            return false;
        };
        let remaining = self.queued_prompts.len();
        self.action = if remaining > 0 {
            Some(format!("予約入力を実行中 残り{remaining}件"))
        } else {
            Some("予約入力を実行中".to_string())
        };
        self.push_log(CodexLogKind::System, "予約した入力を実行します");
        self.run_submitted_line(submitted);
        true
    }

    fn spawn_exec_process(&self, prompt: &str) -> Result<Child> {
        let mut cmd = Command::new(&self.codex_bin);
        for arg in codex_exec_args(self.thread_id.as_deref(), &self.cwd) {
            cmd.arg(arg);
        }

        cmd.current_dir(&self.cwd);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.env("ADDNESS_TUI_CODEX", "1");
        cmd.env("ADDNESS_GOAL_ID", &self.goal_id);
        cmd.env("ADDNESS_GOAL_TITLE", &self.goal_title);
        cmd.env("ADDNESS_GOAL_STATUS", &self.status_label);
        cmd.env("ADDNESS_GOAL_DOD", &self.dod);
        cmd.env("ADDNESS_TASK_GOAL_ID", &self.goal_id);
        cmd.env("ADDNESS_TASK_GOAL_TITLE", &self.goal_title);
        cmd.env("ADDNESS_PARENT_GOAL_ID", &self.parent_goal_id);
        cmd.env("ADDNESS_PARENT_GOAL_TITLE", &self.parent_goal_title);
        cmd.env("ADDNESS_BIN", &self.addness_bin);

        let mut child = cmd.spawn().context("codex exec の起動に失敗しました")?;

        if let Some(mut stdin) = child.stdin.take()
            && let Err(e) = stdin.write_all(prompt.as_bytes())
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e).context("codex exec へのプロンプト送信に失敗しました");
        }

        let stdout = child
            .stdout
            .take()
            .context("codex exec stdout の取得に失敗しました")?;
        let stderr = child
            .stderr
            .take()
            .context("codex exec stderr の取得に失敗しました")?;

        spawn_line_reader(stdout, self.tx.clone(), false);
        spawn_line_reader(stderr, self.tx.clone(), true);

        Ok(child)
    }

    fn kill_current_turn(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
        self.turn_running = false;
        self.current_command = None;
        self.current_command_started_at = None;
        self.streaming_assistant_index = None;
        self.pending_decision = None;
        self.current_turn_prompt = None;
        self.push_log(CodexLogKind::System, "Codex ターンを中断しました");
    }

    /// ユーザーが codex に最後に送信した入力行。
    pub fn last_prompt(&self) -> Option<&str> {
        self.input_state.last_prompt.as_deref()
    }

    /// ユーザーが codex 内で `/exit` を送信し、その結果プロセスも終了しているか。
    pub fn should_close_after_exit_command(&self) -> bool {
        self.finished && self.input_state.exit_command_sent && !self.turn_running
    }

    /// 最初の実依頼を一度だけ body へ自動記録する。記録済み（or 失敗済み）なら None。
    /// 失敗しても再試行はせず、終了/中断時の記録に委ねる（best-effort）。
    pub fn prompt_needs_body_record(&self) -> Option<&str> {
        if self.body_record_done
            || self.pending_task_prompt.is_some()
            || self.task_goal_creation_started
        {
            return None;
        }
        self.last_prompt()
    }

    /// 最初の実依頼の自動記録を済み（再試行しない）として扱う。
    pub fn mark_body_recorded_prompt(&mut self) {
        self.body_record_done = true;
    }

    /// 初回ユーザー依頼を子ゴール作成ジョブへ渡す。ジョブ開始中は再度取り出さない。
    pub fn take_pending_task_goal(&mut self) -> Option<PendingCodexTaskGoal> {
        if self.task_goal_creation_started {
            return None;
        }
        let prompt = self.pending_task_prompt.clone()?;
        self.task_goal_creation_started = true;
        self.action = Some("子ゴール作成中".to_string());
        Some(PendingCodexTaskGoal {
            parent_goal_id: self.parent_goal_id.clone(),
            parent_goal_title: self.parent_goal_title.clone(),
            prompt,
            cwd: self.cwd.clone(),
        })
    }

    /// 作成した子ゴールを以後の Codex 作業対象にして、保留していた初回依頼を実行する。
    pub fn start_pending_prompt_with_task_goal(
        &mut self,
        goal_id: String,
        goal_title: String,
        dod: String,
        status_label: String,
        message: String,
    ) {
        let Some(prompt) = self.pending_task_prompt.take() else {
            self.task_goal_creation_started = false;
            return;
        };
        self.goal_id = goal_id;
        self.goal_title = goal_title;
        self.status_label = status_label;
        self.set_dod(dod);
        self.session_log_path = codex_session_log_path(&self.goal_id);
        self.session_record_count = 0;
        self.loaded_history_count = 0;
        self.task_goal_creation_started = false;
        self.action = Some("子ゴールに文脈を切替".to_string());
        self.push_log(CodexLogKind::System, message);
        self.record_and_run_user_line(prompt);
    }

    /// 子ゴール作成に失敗した場合は親ゴールのまま、保留していた依頼を止めずに実行する。
    pub fn start_pending_prompt_without_task_goal(&mut self, message: String) {
        let Some(prompt) = self.pending_task_prompt.take() else {
            self.task_goal_creation_started = false;
            return;
        };
        self.task_goal_creation_started = false;
        self.action = Some("親ゴールで実行".to_string());
        self.push_log(CodexLogKind::Error, message);
        self.record_and_run_user_line(prompt);
    }

    /// codex プロセスを終了させる（ペインを閉じる時に呼ぶ）。
    /// kill 後に wait してゾンビプロセス化を防ぐ。
    pub fn kill(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
        self.turn_running = false;
        self.current_command = None;
        self.current_command_started_at = None;
        self.pending_decision = None;
        self.current_turn_prompt = None;
        self.queued_prompts.clear();
        self.pending_task_prompt = None;
        self.task_goal_creation_started = false;
    }

    /// DoD を更新する。内容が変わった場合のみ項目・判定を作り直し `true` を返す。
    pub fn set_dod(&mut self, dod: String) -> bool {
        if dod == self.dod {
            return false;
        }
        self.dod = dod;
        self.dod_items = split_dod_items(&self.dod);
        self.dod_checks = vec![None; self.dod_items.len()];
        true
    }

    /// DoD 自動判定の結果（項目インデックス → 達成可否）を反映する。
    pub fn apply_dod_results(&mut self, results: &[(usize, bool)]) {
        for &(i, met) in results {
            if let Some(slot) = self.dod_checks.get_mut(i) {
                *slot = Some(met);
            }
        }
    }
}

#[derive(Default)]
struct CodexInputState {
    line: String,
    exit_command_sent: bool,
    last_prompt: Option<String>,
}

impl CodexInputState {
    fn record_submitted(&mut self, submitted: &str) {
        if !submitted.is_empty() && submitted != "/exit" {
            self.last_prompt = Some(submitted.to_string());
        }
        if submitted == "/exit" {
            self.exit_command_sent = true;
        }
    }

    fn observe_key(&mut self, key: KeyEvent) -> Option<String> {
        if key.modifiers.contains(KeyModifiers::ALT) {
            return None;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if let KeyCode::Char('c' | 'C' | 'u' | 'U') = key.code {
                self.line.clear();
            }
            return None;
        }

        match key.code {
            KeyCode::Char(c) => self.line.push(c),
            KeyCode::Backspace => {
                self.line.pop();
            }
            KeyCode::Enter => {
                let submitted = self.line.trim().to_string();
                self.line.clear();
                return Some(submitted);
            }
            KeyCode::Esc => {
                self.line.clear();
            }
            _ => {}
        }
        None
    }
}

fn spawn_line_reader<R>(reader: R, tx: Sender<CodexProcessEvent>, stderr: bool)
where
    R: std::io::Read + Send + 'static,
{
    std::thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            let Ok(line) = line else {
                break;
            };
            let event = if stderr {
                CodexProcessEvent::Stderr(line)
            } else {
                CodexProcessEvent::Stdout(line)
            };
            if tx.send(event).is_err() {
                break;
            }
        }
    });
}

fn normalize_submitted_line(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

enum AddnessActionKind {
    Read,
    Write,
}

fn addness_command_rest(text: &str) -> Option<&str> {
    if let Some(idx) = text.find("addness ") {
        return Some(text[idx + "addness ".len()..].trim());
    }

    for marker in [
        "\"$ADDNESS_BIN\" ",
        "'$ADDNESS_BIN' ",
        "$ADDNESS_BIN ",
        "${ADDNESS_BIN} ",
    ] {
        if let Some(idx) = text.find(marker) {
            return Some(text[idx + marker.len()..].trim());
        }
    }

    None
}

fn rest_has_flag(rest: &str, flag: &str) -> bool {
    let with_eq = format!("{flag}=");
    rest.split_whitespace()
        .any(|part| part == flag || part.starts_with(&with_eq))
}

fn addness_activity_summary(command: &str, output: Option<&str>) -> Option<String> {
    let rest = addness_command_rest(command).unwrap_or(command);
    let mut parts = rest.split_whitespace();
    let first = parts.next().unwrap_or("");
    let second = parts.next().unwrap_or("");
    let json_summary = output.and_then(addness_json_output_summary);

    let kind = match (first, second) {
        ("goal", "update") if rest_has_flag(rest, "--body") => "body変更",
        ("goal", "update") if rest_has_flag(rest, "--description") => "DoD変更",
        ("goal", "update") if rest_has_flag(rest, "--status") => "ステータス変更",
        ("goal", "update") if rest_has_flag(rest, "--title") => "タイトル変更",
        ("goal", "create") => "子ゴール追加",
        ("comment", "create") => "コメント追加",
        ("link", "progress") => "進捗リンク追加",
        ("link", "pr") => "PRリンク追加",
        ("deliverable", _) => "成果物変更",
        ("goal", "get" | "list" | "children" | "tree" | "search" | "siblings") => "ゴール読込",
        ("comment", _) => "コメント読込",
        ("status" | "summary", _) => "状態読込",
        _ => {
            if looks_like_addness_command_text(command) {
                "Addness操作"
            } else {
                return None;
            }
        }
    };

    let detail = json_summary
        .map(|summary| format!(": {summary}"))
        .unwrap_or_default();
    Some(format!("Addness {kind}{detail}"))
}

fn looks_like_addness_command_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("addness")
        || lower.contains("$addness_bin")
        || lower.contains(" goal ")
        || lower.contains(" comment ")
        || lower.contains(" deliverable ")
        || lower.contains(" link ")
}

fn addness_json_output_summary(output: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(output.trim()).ok()?;
    if let Some(title) = addness_json_title(&value) {
        return Some(compact_tool_text(title));
    }
    match value {
        Value::Array(items) => Some(format!("{}件", items.len())),
        Value::Object(map) => Some(format!("{}キー", map.len())),
        _ => Some("JSON".to_string()),
    }
}

fn addness_json_title(value: &Value) -> Option<&str> {
    value
        .get("title")
        .and_then(Value::as_str)
        .or_else(|| value.get("name").and_then(Value::as_str))
        .or_else(|| {
            value
                .get("goal")
                .and_then(|goal| goal.get("title"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            value
                .get("data")
                .and_then(|data| data.get("title"))
                .and_then(Value::as_str)
        })
}

fn rest_has_any_flag(rest: &str, flags: &[&str]) -> bool {
    flags.iter().any(|flag| rest_has_flag(rest, flag))
}

fn rest_flag_value(rest: &str, flag: &str) -> Option<String> {
    let with_eq = format!("{flag}=");
    let mut parts = rest.split_whitespace();
    while let Some(part) = parts.next() {
        if part == flag {
            return parts.next().map(trim_flag_value);
        }
        if let Some(value) = part.strip_prefix(&with_eq) {
            return Some(trim_flag_value(value));
        }
    }
    None
}

fn trim_flag_value(value: &str) -> String {
    value.trim_matches('"').trim_matches('\'').to_string()
}

/// `addness <サブコマンド…>` 文字列を「いま何をしているか」の表示ラベルへ変換する。
fn action_label(rest: &str) -> (String, AddnessActionKind) {
    let mut it = rest.split_whitespace();
    let a = it.next().unwrap_or("");
    let b = it.next().unwrap_or("");
    match (a, b) {
        ("goal", "create") => ("子ゴールを書込中".to_string(), AddnessActionKind::Write),
        ("goal", "update") if rest_has_any_flag(rest, &["--body", "--body-file"]) => {
            ("現状(body)を書込中".to_string(), AddnessActionKind::Write)
        }
        ("goal", "update") if rest_has_any_flag(rest, &["--description", "--description-file"]) => {
            ("方針(DoD)を書込中".to_string(), AddnessActionKind::Write)
        }
        ("goal", "update") if rest_has_flag(rest, "--status") => {
            ("ステータスを書込中".to_string(), AddnessActionKind::Write)
        }
        ("goal", "update") if rest_has_flag(rest, "--title") => {
            ("タイトルを書込中".to_string(), AddnessActionKind::Write)
        }
        ("goal", "update") if rest_has_any_flag(rest, &["--due-date", "--clear-due-date"]) => {
            ("期限を書込中".to_string(), AddnessActionKind::Write)
        }
        ("goal", "update") => ("ゴールを書込中".to_string(), AddnessActionKind::Write),
        ("goal", "get" | "list" | "children" | "tree" | "search" | "siblings") => {
            ("ゴール文脈を読込中".to_string(), AddnessActionKind::Read)
        }
        ("comment", "create" | "update" | "delete" | "resolve" | "unresolve" | "react") => {
            ("コメントを書込中".to_string(), AddnessActionKind::Write)
        }
        ("comment", _) => ("コメントを参照中".to_string(), AddnessActionKind::Read),
        ("notification", "send") => {
            let label = match rest_flag_value(rest, "--kind").as_deref() {
                Some("done") => "作業完了通知を送信中",
                Some("review") => "確認依頼通知を送信中",
                Some("blocked") => "ブロック通知を送信中",
                _ => "通知コメントを送信中",
            };
            (label.to_string(), AddnessActionKind::Write)
        }
        ("notification", _) => ("通知を処理中".to_string(), AddnessActionKind::Read),
        ("link", "pr") => ("PRリンクを書込中".to_string(), AddnessActionKind::Write),
        ("link", "progress") => ("進捗コメントを書込中".to_string(), AddnessActionKind::Write),
        ("link" | "deliverable", _) => ("成果物を書込中".to_string(), AddnessActionKind::Write),
        ("today", _) => ("今日のtodoを更新中".to_string(), AddnessActionKind::Write),
        ("status" | "summary", _) => ("状況を確認中".to_string(), AddnessActionKind::Read),
        (cmd, _) if !cmd.is_empty() => (format!("addness {cmd} 実行中"), AddnessActionKind::Read),
        _ => ("addness を実行中".to_string(), AddnessActionKind::Read),
    }
}

/// DoD テキストを行単位の項目リストへ分割する（空行は除外）。
fn split_dod_items(dod: &str) -> Vec<String> {
    dod.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

fn codex_session_log_path(goal_id: &str) -> Option<PathBuf> {
    dirs::home_dir().map(|home| {
        home.join(".addness")
            .join(CODEX_SESSION_HISTORY_DIR)
            .join(format!("{}.jsonl", safe_path_component(goal_id)))
    })
}

fn safe_path_component(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('_');
        }
        if out.len() >= 96 {
            break;
        }
    }
    if out.is_empty() {
        "codex-session".to_string()
    } else {
        out
    }
}

fn compact_home_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(stripped) = path.strip_prefix(&home)
    {
        return format!("~/{}", stripped.display());
    }
    path.display().to_string()
}

fn load_codex_session(path: &Path) -> Result<LoadedCodexSession> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LoadedCodexSession {
                log: Vec::new(),
                record_count: 0,
            });
        }
        Err(e) => {
            return Err(e).with_context(|| format!("履歴ファイルを開けません: {}", path.display()));
        }
    };

    let mut log = Vec::new();
    let mut record_count = 0usize;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        record_count = record_count.saturating_add(1);
        let Ok(record) = serde_json::from_str::<CodexSessionRecord>(&line) else {
            continue;
        };
        match record {
            CodexSessionRecord::Log { kind, text } => log.push(CodexLogLine::new(kind, text)),
            CodexSessionRecord::UpdateTurn { turn, text } => {
                update_turn_log_line(&mut log, turn, text);
            }
            CodexSessionRecord::AssistantDelta { text } => {
                if let Some(last) = log.last_mut()
                    && last.kind == CodexLogKind::Assistant
                {
                    last.text.push_str(&text);
                    continue;
                }
                log.push(CodexLogLine::new(CodexLogKind::Assistant, text));
            }
            CodexSessionRecord::RawEvent { .. } => {}
        }
        if log.len() > CODEX_SESSION_HISTORY_MAX_LOG_LINES {
            let removed = log.len() - CODEX_SESSION_HISTORY_MAX_LOG_LINES;
            log.drain(0..removed);
        }
    }

    Ok(LoadedCodexSession { log, record_count })
}

fn append_codex_session_record(path: &Path, record: &CodexSessionRecord) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("履歴ディレクトリを作成できません: {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("履歴ファイルを開けません: {}", path.display()))?;
    serde_json::to_writer(&mut file, record).context("履歴レコードのJSON化に失敗しました")?;
    writeln!(file).context("履歴レコードの書き込みに失敗しました")?;
    Ok(())
}

fn trim_codex_session_log(path: &Path) -> Result<usize> {
    let file = File::open(path)
        .with_context(|| format!("履歴ファイルを開けません: {}", path.display()))?;
    let lines = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();

    let mut kept = Vec::new();
    let mut bytes = 0u64;
    for line in lines.iter().rev() {
        let line_bytes = line.len() as u64 + 1;
        if kept.len() >= CODEX_SESSION_HISTORY_MAX_RECORDS {
            break;
        }
        if !kept.is_empty() && bytes.saturating_add(line_bytes) > CODEX_SESSION_HISTORY_MAX_BYTES {
            break;
        }
        kept.push(line.clone());
        bytes = bytes.saturating_add(line_bytes);
    }
    kept.reverse();
    let mut file = File::create(path)
        .with_context(|| format!("履歴ファイルを切り詰められません: {}", path.display()))?;
    for line in &kept {
        writeln!(file, "{line}").context("履歴ファイルの再書き込みに失敗しました")?;
    }
    Ok(kept.len())
}

fn rendered_log_line_height(text: &str, width: usize) -> usize {
    let content_width = width.saturating_sub(CODEX_LOG_PREFIX_WIDTH).max(1);
    let normalized = text.replace('\r', "");
    let mut total = 0usize;
    for segment in normalized.split('\n') {
        let width = UnicodeWidthStr::width(segment);
        total += width.saturating_add(content_width - 1) / content_width;
        if segment.is_empty() {
            total += 1;
        }
    }
    total.max(1)
}

fn contains_case_insensitive(text: &str, lowercase_query: &str) -> bool {
    if lowercase_query.is_empty() {
        return true;
    }
    if lowercase_query.is_ascii() {
        let needle = lowercase_query.as_bytes();
        return text
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle));
    }
    text.to_lowercase().contains(lowercase_query)
}

fn compact_turn_prompt(prompt: &str) -> String {
    let normalized = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_CHARS: usize = 64;
    if normalized.chars().count() <= MAX_CHARS {
        return normalized;
    }
    let mut out = normalized.chars().take(MAX_CHARS).collect::<String>();
    out.push_str("...");
    out
}

fn turn_number_from_label(label: &str) -> Option<usize> {
    let rest = label.strip_prefix("Turn ")?;
    let digits = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits.parse().ok()
}

fn update_turn_log_line(log: &mut [CodexLogLine], turn: usize, text: String) -> bool {
    let Some(line) = log.iter_mut().find(|line| {
        line.kind == CodexLogKind::Turn && turn_number_from_label(&line.text) == Some(turn)
    }) else {
        return false;
    };
    if line.text == text {
        return false;
    }
    line.text = text;
    true
}

fn summarize_turn_title(log: &[CodexLogLine], turn: usize) -> Option<String> {
    let (start, end) = turn_log_bounds(log, turn)?;
    let lines = &log[start + 1..end];
    summarize_turn_assistant_title(lines).or_else(|| summarize_turn_tool_title(lines))
}

fn turn_log_bounds(log: &[CodexLogLine], turn: usize) -> Option<(usize, usize)> {
    let start = log.iter().position(|line| {
        line.kind == CodexLogKind::Turn && turn_number_from_label(&line.text) == Some(turn)
    })?;
    let end = log[start + 1..]
        .iter()
        .position(|line| line.kind == CodexLogKind::Turn)
        .map(|offset| start + 1 + offset)
        .unwrap_or(log.len());
    Some((start, end))
}

fn summarize_turn_assistant_title(lines: &[CodexLogLine]) -> Option<String> {
    lines
        .iter()
        .rev()
        .filter(|line| line.kind == CodexLogKind::Assistant)
        .flat_map(|line| line.text.lines())
        .find_map(clean_assistant_title_line)
}

fn clean_assistant_title_line(line: &str) -> Option<String> {
    let title = line
        .trim()
        .trim_start_matches(['#', '-', '*', '•', ' '])
        .trim();
    if title.is_empty() || title.starts_with("```") {
        return None;
    }
    if matches!(
        title,
        "対応しました"
            | "対応しました。"
            | "実装しました"
            | "実装しました。"
            | "完了しました"
            | "完了しました。"
    ) {
        return None;
    }
    Some(compact_turn_prompt(title))
}

fn summarize_turn_tool_title(lines: &[CodexLogLine]) -> Option<String> {
    lines
        .iter()
        .rev()
        .filter(|line| line.kind == CodexLogKind::Tool)
        .find_map(|line| summarize_turn_tool_line(&line.text))
}

fn summarize_turn_tool_line(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("*** update file:")
        || lower.contains("*** add file:")
        || lower.contains("*** delete file:")
        || lower.contains("apply_patch")
        || lower.starts_with("edit ")
    {
        return Some(
            extract_code_edit_path(text)
                .map(|path| format!("コード編集: {path}"))
                .unwrap_or_else(|| "コード編集".to_string()),
        );
    }
    for (needle, label) in [
        ("cargo fmt", "フォーマット確認"),
        ("cargo test", "テスト実行"),
        ("cargo clippy", "clippy確認"),
        ("cargo build", "ビルド確認"),
        ("git diff", "差分確認"),
        ("git status", "差分確認"),
        (" goal get ", "Addnessゴール確認"),
        (" goal update ", "Addnessゴール更新"),
        (" notification send ", "Addness通知送信"),
        (" deliverable ", "成果物更新"),
    ] {
        if lower.contains(needle) {
            return Some(label.to_string());
        }
    }
    let first = text.lines().next()?.trim();
    let first = strip_turn_tool_state(first);
    (!first.is_empty()).then(|| compact_turn_prompt(&compact_tool_text(first)))
}

fn strip_turn_tool_state(text: &str) -> &str {
    for state in ["RUNNING", "EDIT", "DIFF", "FAIL", "OK"] {
        if let Some(rest) = text.strip_prefix(state)
            && rest.chars().next().is_some_and(char::is_whitespace)
        {
            return rest.trim_start();
        }
    }
    text
}

fn extract_code_edit_path(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let trimmed = line.trim();
        [
            "*** Update File: ",
            "*** Add File: ",
            "*** Delete File: ",
            "*** Move to: ",
        ]
        .into_iter()
        .find_map(|prefix| trimmed.strip_prefix(prefix).map(str::to_string))
    })
}

fn summarize_implemented_work(log: &[CodexLogLine]) -> Vec<String> {
    log.iter()
        .rev()
        .filter(|line| line.kind == CodexLogKind::Assistant)
        .flat_map(|line| line.text.lines())
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("```"))
        .take(3)
        .map(compact_tool_text)
        .collect()
}

fn summarize_checks(log: &[CodexLogLine]) -> Vec<String> {
    let mut out = Vec::new();
    for line in log.iter().filter(|line| line.kind == CodexLogKind::Tool) {
        let lower = line.text.to_ascii_lowercase();
        if !(lower.contains("cargo fmt")
            || lower.contains("cargo clippy")
            || lower.contains("cargo test")
            || lower.contains("cargo build")
            || lower.contains("git diff"))
        {
            continue;
        }
        let summary = line
            .text
            .lines()
            .find(|text| {
                text.contains("test result:")
                    || text.contains("Finished ")
                    || text.contains("files changed")
            })
            .or_else(|| line.text.lines().find(|text| text.contains("exit ")))
            .unwrap_or_else(|| line.text.lines().next().unwrap_or(""));
        let summary = compact_tool_text(summary);
        if !summary.is_empty() && !out.iter().any(|seen| seen == &summary) {
            out.push(summary);
        }
    }
    out
}

fn summarize_remaining_work(log: &[CodexLogLine]) -> Vec<String> {
    let mut out = Vec::new();
    for line in log.iter().rev() {
        if matches!(line.kind, CodexLogKind::Error) {
            out.push(compact_tool_text(&line.text));
        }
        if out.len() >= 3 {
            break;
        }
    }
    if out.is_empty() {
        out.push("未記録".to_string());
    }
    out
}

fn string_at_any(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::to_string)
}

fn nested_error_message(value: &Value) -> Option<String> {
    value
        .get("error")
        .and_then(|error| {
            error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
        })
        .map(str::to_string)
}

fn decision_banner(event_type: &str, text: Option<&str>) -> Option<CodexDecisionBanner> {
    let event_lower = event_type.to_ascii_lowercase();
    let event_requests_decision = [
        "approval",
        "confirm",
        "permission",
        "decision",
        "requires_action",
        "input_requested",
        "user_input",
        "consent",
    ]
    .iter()
    .any(|needle| event_lower.contains(needle));

    let message = text
        .map(compact_tool_text)
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| event_type.to_string());
    let text_lower = message.to_lowercase();
    let kind = if [
        "danger",
        "dangerous",
        "destructive",
        "rm ",
        "delete",
        "reset --hard",
        "force",
        "破壊",
        "危険",
    ]
    .iter()
    .any(|needle| text_lower.contains(needle))
    {
        CodexDecisionKind::Dangerous
    } else if ["permission", "forbidden", "403", "権限", "許可"]
        .iter()
        .any(|needle| text_lower.contains(needle) || event_lower.contains(needle))
    {
        CodexDecisionKind::Permission
    } else if ["approval", "approve", "承認"]
        .iter()
        .any(|needle| text_lower.contains(needle) || event_lower.contains(needle))
    {
        CodexDecisionKind::Approval
    } else {
        CodexDecisionKind::YesNo
    };
    let text_requests_decision = [
        "yes/no",
        "y/n",
        "approve",
        "approval",
        "allow",
        "deny",
        "confirm",
        "permission",
        "proceed",
        "承認",
        "許可",
        "確認",
        "続行",
    ]
    .iter()
    .any(|needle| text_lower.contains(needle));

    (event_requests_decision || text_requests_decision)
        .then_some(CodexDecisionBanner::new(kind, message))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolDisplay {
    label: String,
    action_text: Option<String>,
    command_text: Option<String>,
    output_text: Option<String>,
}

fn tool_display(event_type: &str, value: &Value) -> Option<ToolDisplay> {
    if !is_tool_like_event(event_type, value) {
        return None;
    }

    let name = tool_name(value).unwrap_or_else(|| event_type.to_string());
    let display_name = tool_display_name(event_type, &name);
    let command = command_text(value).map(|text| compact_tool_text(&text));
    let output = tool_output_text(value).map(|text| compact_tool_text(&text));
    let exit_code = scalar_field_text(value, &["exit_code", "exitCode"]);
    let duration = scalar_field_text(value, &["duration", "duration_ms", "durationMs"]);
    let cwd = scalar_field_text(value, &["cwd", "workdir", "working_directory", "codex_cwd"]);

    let mut attrs = Vec::new();
    if let Some(exit_code) = exit_code.as_deref() {
        attrs.push(format!("exit {exit_code}"));
    }
    if let Some(duration) = duration.as_deref() {
        attrs.push(format!("duration {duration}"));
    }

    let suffix = if attrs.is_empty() {
        String::new()
    } else {
        format!(" ({})", attrs.join(", "))
    };

    let Some(primary) = command.as_deref().or(output.as_deref()) else {
        return Some(ToolDisplay {
            label: format!("{display_name}{suffix}"),
            action_text: None,
            command_text: None,
            output_text: None,
        });
    };

    let is_code_edit = is_code_edit_tool(event_type, &name, command.as_deref(), output.as_deref());
    let state = tool_state_label(
        event_type,
        exit_code.as_deref(),
        command.as_deref(),
        is_code_edit,
    );
    let mut label = format!("{state} {primary}");
    if !display_name.is_empty() && !primary.contains(&display_name) {
        label.push_str(&format!("  [{display_name}]"));
    }
    if !suffix.is_empty() {
        label.push_str(&suffix);
    }
    if let Some(cwd) = cwd.as_deref()
        && !cwd.is_empty()
        && command.is_some()
    {
        label.push_str(&format!("  [cwd: {cwd}]"));
    }
    if let (Some(command), Some(output)) = (command.as_deref(), output.as_deref())
        && !output.is_empty()
        && output != command
    {
        label.push('\n');
        label.push_str(output);
    }

    Some(ToolDisplay {
        label,
        action_text: command.clone(),
        command_text: command,
        output_text: output,
    })
}

fn tool_state_label(
    event_type: &str,
    exit_code: Option<&str>,
    command: Option<&str>,
    is_code_edit: bool,
) -> &'static str {
    let lower_event = event_type.to_ascii_lowercase();
    let lower_command = command.unwrap_or("").to_ascii_lowercase();
    if lower_event.contains("begin") || lower_event.contains("start") {
        return "RUNNING";
    }
    if lower_command.contains("git diff") || lower_command.contains("git status") {
        return "DIFF";
    }
    if let Some(code) = exit_code {
        if code == "0" {
            return if is_code_edit { "EDIT" } else { "OK" };
        }
        return "FAIL";
    }
    if lower_event.contains("failed") || lower_event.contains("error") {
        return "FAIL";
    }
    if is_code_edit {
        return "EDIT";
    }
    if is_tool_completion_event(event_type) {
        return "OK";
    }
    "RUNNING"
}

fn is_code_edit_tool(
    event_type: &str,
    name: &str,
    command: Option<&str>,
    output: Option<&str>,
) -> bool {
    let event_type = event_type.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    if name == "apply_patch"
        || name.contains("edit")
        || name.contains("write_file")
        || name.contains("file_write")
        || name.contains("update_file")
    {
        return true;
    }
    [Some(event_type.as_str()), command, output]
        .into_iter()
        .flatten()
        .any(looks_like_code_edit_text)
}

fn looks_like_code_edit_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("apply_patch")
        || text.contains("*** Begin Patch")
        || text.contains("*** Update File:")
        || text.contains("*** Add File:")
        || text.contains("*** Delete File:")
}

fn is_tool_completion_event(event_type: &str) -> bool {
    let event_type = event_type.to_ascii_lowercase();
    event_type.contains("end")
        || event_type.contains("completed")
        || event_type.contains("finished")
        || event_type.contains("failed")
        || event_type.contains("result")
}

fn tool_display_name(event_type: &str, name: &str) -> String {
    if event_type == "response_item" || event_type == "event" || event_type == name {
        name.to_string()
    } else {
        format!("{event_type}/{name}")
    }
}

fn is_tool_like_event(event_type: &str, value: &Value) -> bool {
    let event_type = event_type.to_ascii_lowercase();
    if event_type.contains("exec")
        || event_type.contains("tool")
        || event_type.contains("function")
        || event_type.contains("mcp")
    {
        return true;
    }
    value_has_tool_hint(value)
}

fn value_has_tool_hint(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(is_tool_like_name)
            {
                return true;
            }
            if map
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(is_tool_like_kind)
            {
                return true;
            }
            if map
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(is_tool_like_kind)
            {
                return true;
            }
            if map.contains_key("codex_command")
                || map.contains_key("codex_parsed_cmd")
                || map.contains_key("parsed_cmd")
            {
                return true;
            }
            if map.contains_key("cmd")
                && (map.contains_key("workdir")
                    || map.contains_key("cwd")
                    || map.contains_key("yield_time_ms")
                    || map.contains_key("sandbox_permissions"))
            {
                return true;
            }

            for key in [
                "payload",
                "item",
                "event",
                "data",
                "result",
                "call",
                "tool_call",
                "toolCall",
                "function",
            ] {
                if let Some(child) = map.get(key)
                    && value_has_tool_hint(child)
                {
                    return true;
                }
            }
            false
        }
        Value::Array(values) => values.iter().any(value_has_tool_hint),
        _ => false,
    }
}

fn is_tool_like_kind(kind: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    kind.contains("tool")
        || kind.contains("exec")
        || kind.contains("mcp")
        || kind == "function_call"
        || kind == "local_shell_call"
        || kind == "shell_call"
}

fn is_tool_like_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("exec")
        || name.contains("tool")
        || name.contains("mcp")
        || name == "apply_patch"
        || name == "shell_command"
        || name == "run_command"
        || name == "terminal"
        || name == "bash"
}

fn tool_name(value: &Value) -> Option<String> {
    recursive_scalar_field_text(
        value,
        &[
            "name",
            "tool_name",
            "toolName",
            "function_name",
            "functionName",
        ],
    )
}

fn first_text_field(value: &Value) -> Option<String> {
    const TEXT_KEYS: &[&str] = &[
        "message", "text", "content", "delta", "summary", "output", "stdout", "stderr",
    ];

    match value {
        Value::String(s) => Some(s.clone()),
        Value::Array(values) => values.iter().find_map(first_text_field),
        Value::Object(map) => {
            for key in TEXT_KEYS {
                if let Some(found) = map.get(*key).and_then(first_text_field)
                    && !found.is_empty()
                {
                    return Some(found);
                }
            }
            for key in [
                "payload",
                "item",
                "event",
                "data",
                "result",
                "call",
                "tool_call",
                "toolCall",
            ] {
                if let Some(found) = map.get(key).and_then(first_text_field)
                    && !found.is_empty()
                {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn command_text(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in [
                "command",
                "cmd",
                "codex_command",
                "parsed_cmd",
                "codex_parsed_cmd",
                "interaction_input",
                "argv",
                "args",
                "arguments",
                "input",
            ] {
                if let Some(text) = map.get(key).and_then(command_text)
                    && !text.is_empty()
                {
                    return Some(text);
                }
            }
            for key in [
                "payload",
                "item",
                "call",
                "tool_call",
                "toolCall",
                "data",
                "event",
                "result",
            ] {
                if let Some(text) = map.get(key).and_then(command_text)
                    && !text.is_empty()
                {
                    return Some(text);
                }
            }
            None
        }
        Value::Array(values) => {
            let parts = values
                .iter()
                .filter_map(command_text)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join(" "))
        }
        Value::String(s) => json_string_value(s)
            .and_then(|value| command_text(&value))
            .or_else(|| Some(s.clone())),
        _ => None,
    }
}

fn tool_output_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Array(values) => values.iter().find_map(tool_output_text),
        Value::Object(map) => {
            for key in [
                "formatted_output",
                "aggregated_output",
                "stdout",
                "stderr",
                "output",
                "delta",
            ] {
                if let Some(found) = map.get(key).and_then(tool_output_text)
                    && !found.is_empty()
                {
                    return Some(found);
                }
            }
            for key in [
                "payload",
                "item",
                "event",
                "data",
                "result",
                "call",
                "tool_call",
                "toolCall",
            ] {
                if let Some(found) = map.get(key).and_then(tool_output_text)
                    && !found.is_empty()
                {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn scalar_field_text(value: &Value, keys: &[&str]) -> Option<String> {
    recursive_scalar_field_text(value, keys).map(|text| compact_tool_text(&text))
}

fn recursive_scalar_field_text(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(text) = map.get(*key).and_then(scalar_value_text)
                    && !text.is_empty()
                {
                    return Some(text);
                }
            }
            for key in [
                "payload",
                "item",
                "event",
                "data",
                "result",
                "call",
                "tool_call",
                "toolCall",
                "function",
                "arguments",
                "params",
                "input",
            ] {
                if let Some(found) = map
                    .get(key)
                    .and_then(|child| recursive_scalar_field_text(child, keys))
                    && !found.is_empty()
                {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(values) => values
            .iter()
            .find_map(|child| recursive_scalar_field_text(child, keys)),
        Value::String(s) => {
            json_string_value(s).and_then(|child| recursive_scalar_field_text(&child, keys))
        }
        _ => None,
    }
}

fn scalar_value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn json_string_value(s: &str) -> Option<Value> {
    let trimmed = s.trim();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

fn compact_tool_text(text: &str) -> String {
    const MAX_CHARS: usize = 2_000;
    let trimmed = text.trim();
    let mut out = String::new();
    for (idx, ch) in trimmed.chars().enumerate() {
        if idx == MAX_CHARS {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

pub fn resume_prompt() -> &'static str {
    "Addnessの対象ゴールを読み、前回の続きから再開してください。bodyの `## Codex作業メモ` / `## Codex決定ログ` / `## PR/Release Traceability`、DoD、子ゴール、コメント、成果物を確認し、前回の続き・未完了・次の一手を短く整理してから進めてください。"
}

fn addness_tui_developer_instructions() -> &'static str {
    r#"Addness TUIから起動されました。

起動直後は Addness CLI を実行せず、ユーザーの最初の入力を待ってください。
軽い挨拶や単純な表示確認には、TUIから渡された軽量コンテキストだけで即応して構いません。

軽量コンテキスト:
- ADDNESS_GOAL_ID: 作業対象ゴールID（通常はTUIが初回依頼から作成した子ゴール）
- ADDNESS_GOAL_TITLE: 作業対象ゴール名
- ADDNESS_GOAL_STATUS: 対象ゴールの現在状態
- ADDNESS_GOAL_DOD: 対象ゴールのDoD/完了基準
- ADDNESS_PARENT_GOAL_ID: TUIでcodexを起動した親ゴールID（ある場合）
- ADDNESS_PARENT_GOAL_TITLE: TUIでcodexを起動した親ゴール名（ある場合）

最初の依頼が「何をするかの相談」「方針検討」「実装」「調査」「レビュー」「PR/リリース」など
プロジェクト固有の判断を含む場合は、返答や作業に入る前に Addness CLI で対象ゴールを想起してください。

想起コマンド:
`"$ADDNESS_BIN" goal get "$ADDNESS_GOAL_ID" --json --with-deliverable --with-comment`

Addness はこの組織/プロジェクト専用の共有DB・長期作業メモリ・引き継ぎ点です。
通常のmemoryは複数プロジェクトの状態が混ざることがあるため、プロジェクト固有の現在地・判断・決定・次の手は
Addnessを真実源として扱ってください。
Codexが普段memoryに保存したくなるプロジェクト固有の情報（決定、前提、落とし穴、重要コマンド、未完了点、次回の前提）は、
通常memoryではなく Addness の body に記録してください。

読み取り時:
1. body、DoD(description/definitionOfDone)、コメント、成果物、子ゴール、作業フォルダ/ブランチを確認する。
2. DoDが空・曖昧・現在の作業に対して不足していないかを見る。
3. 子ゴールが、実際の作業・分担・並列化・サブエージェント化に使える粒度かを見る。
4. 「何をしたいか」の相談だけでも、既存のbody/DoD/子ゴールを踏まえて提案する。

書き込み時:
- 起動しただけでは Addness に書き込まない。
- TUIが初回依頼から作成した子ゴールが `ADDNESS_GOAL_ID` に渡されている場合、通常の作業メモ・決定・PR/成果物はその子ゴールへ集約する。親ゴールを参照したい時だけ `ADDNESS_PARENT_GOAL_ID` を読む。
- 子ゴールの追加作成、body/DoD/コメント/成果物リンクなどのコンテキスト書き込みは、サブエージェント機能が使える場合、最も軽量/低コストの記録専用サブエージェントに委任する。メインエージェントは実装判断と検証に集中し、委任入力は「対象ゴールID」「現在body」「追加したい要約」「更新フィールド」に絞る。
- Addnessに書き込むのは、作業を始めた時、方針を決めた時、重要な発見や制約が分かった時、実装のまとまりが進んだ時、次にやることが変わった時、完了/中断/引き継ぎ前。
- `## Codex自動メモ(機械)`（`<!-- addness:codex:auto-record:start -->` / `<!-- addness:codex:auto-record:end -->` で囲まれた領域）はTUIが自動更新するので読むだけにし、編集しない。あなたが所有・更新するのは `## Codex作業メモ`（判断・方針・次の手を散文で）/`## Codex決定ログ`/`## PR/Release Traceability`。
- body更新前に必ず現bodyを読み、手書きメモや上記の自動メモを消さず、自分の専用ブロックだけを作成・更新する。長文や引用が多い場合は `goal update --body-file` を使う。
- bodyには作業フォルダ、ブランチ、現在の方針、実施中の内容、決定事項、重要な発見、未完了点、次の手をまとめる。ツール実行ログを逐語的に溜めない。
- 決定事項は body の `## Codex決定ログ` に追記する。形式は `- YYYY-MM-DD HH:MM - 決定: ... / 理由: ... / 影響: ...`。
- 再開を求められたら body の `## Codex作業メモ`、`## Codex決定ログ`、`## PR/Release Traceability`、DoD、子ゴール、コメント、成果物を読んで、前回の続き・未完了・次の一手を短く整理してから進める。
- PRを作成したら `"$ADDNESS_BIN" link pr --goal "$ADDNESS_GOAL_ID" --url "<PR_URL>" --name "<name>" --json` で紐づける。
- tag/releaseを作成したら `"$ADDNESS_BIN" deliverable add --goal "$ADDNESS_GOAL_ID" --link-url "<release-or-tag-url>" --name "Release <version>" --json` で紐づけ、body の `## PR/Release Traceability` にPR・tag・release URL・CI結果を残す。
- ユーザーに通知したい時（作業完了、確認依頼、ブロック中など）は `"$ADDNESS_BIN" notification send --kind done --body "<通知内容>" --json` のように送る。これは対象ゴールへの通知用コメントを作り、同時にTUIが動いている端末へ通知する。完了は `--kind done`、確認依頼は `--kind review`、ブロック中は `--kind blocked`。必要なら `--mention <ORG_MEMBER_ID>` を付ける。
- 実装速度を落とさないため、Addness更新は節目でまとめる。毎コマンド・毎小変更の記録はしない。
- サブエージェントが使えない場合だけ、メインエージェントが短い差分で body/DoD/子ゴールを更新する。
- DoDが不十分なら、足りない観点を短く整理してユーザーに確認し、合意できたら `"$ADDNESS_BIN" goal update "$ADDNESS_GOAL_ID" --description "..." --json` で更新する。
- 子ゴール分解が不十分なら、作業に必要な粒度へ子ゴールを作成または更新する。
- サブエージェントが必要な場合は、必要な作業単位を子ゴールとして作成または更新する。子ゴールのtitleは作業名、descriptionはそのサブエージェントのDoD、bodyは入力情報・作業フォルダ・ブランチ・期待成果物・現在地を書く。
- コメントは構造化フィールドに置けない質問や補足だけに使う。

Addnessを読んだ場合は、何を読んだか、次に進めることを短く共有してください。
Addnessを更新した場合は、body/DoD/子ゴール/コメント/成果物/通知のどれを更新したかを短く共有してください。"#
}

enum CodexConfigKey {
    DeveloperInstructions,
}

impl CodexConfigKey {
    fn as_str(&self) -> &'static str {
        match self {
            CodexConfigKey::DeveloperInstructions => "developer_instructions",
        }
    }
}

fn codex_config_arg(key: CodexConfigKey, value: &str) -> String {
    format!("{}={}", key.as_str(), toml_basic_string(value))
}

fn codex_exec_args(thread_id: Option<&str>, cwd: &str) -> Vec<String> {
    let developer_instructions = codex_config_arg(
        CodexConfigKey::DeveloperInstructions,
        addness_tui_developer_instructions(),
    );
    if let Some(thread_id) = thread_id {
        return vec![
            "exec".to_string(),
            "resume".to_string(),
            "--json".to_string(),
            "-c".to_string(),
            developer_instructions,
            thread_id.to_string(),
            "-".to_string(),
        ];
    }

    vec![
        "exec".to_string(),
        "--json".to_string(),
        "--color".to_string(),
        "never".to_string(),
        "-s".to_string(),
        "workspace-write".to_string(),
        "-C".to_string(),
        cwd.to_string(),
        "-c".to_string(),
        developer_instructions,
        "-".to_string(),
    ]
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

/// DoD 自動判定で codex に強制する出力 JSON Schema。
/// `{ "results": [ { "index": <int>, "met": <bool> } ] }` の形を要求する。
pub fn dod_assessment_schema() -> String {
    r#"{
  "type": "object",
  "additionalProperties": false,
  "required": ["results"],
  "properties": {
    "results": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["index", "met"],
        "properties": {
          "index": { "type": "integer", "minimum": 0 },
          "met": { "type": "boolean" }
        }
      }
    }
  }
}"#
    .to_string()
}

/// DoD 自動判定用のプロンプトを組み立てる。各項目に番号を振って提示する。
pub fn build_dod_assessment_prompt(items: &[String]) -> String {
    let mut listed = String::new();
    for (i, item) in items.iter().enumerate() {
        listed.push_str(&format!("{i}: {item}\n"));
    }
    format!(
        r#"あなたはコードレビュー担当です。コードの変更は一切行わないでください（read-only）。

現在のリポジトリの作業ツリーの状態（`git diff HEAD` や関連ファイルの内容）を調べ、
以下の各「完了基準(DoD)項目」が**現時点で満たされているか**を判定してください。

DoD項目（番号: 内容）:
{listed}
判定結果は、指定された JSON Schema に厳密に従って出力してください。
各項目について index（番号）と met（満たされていれば true、そうでなければ false）を返します。
確証が持てない項目は met=false としてください。"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn input_state_records_last_prompt_on_enter() {
        let mut state = CodexInputState::default();
        for ch in "  cargo test を実行して  ".chars() {
            assert!(state.observe_key(key(KeyCode::Char(ch))).is_none());
        }
        let submitted = state.observe_key(key(KeyCode::Enter));
        assert_eq!(submitted.as_deref(), Some("cargo test を実行して"));
        state.record_submitted(submitted.as_deref().unwrap());

        assert_eq!(state.last_prompt.as_deref(), Some("cargo test を実行して"));
    }

    #[test]
    fn input_state_detects_exit_command_on_enter() {
        let mut state = CodexInputState::default();
        for ch in "/exit".chars() {
            assert!(state.observe_key(key(KeyCode::Char(ch))).is_none());
        }
        let submitted = state.observe_key(key(KeyCode::Enter)).unwrap();
        state.record_submitted(&submitted);

        assert!(state.exit_command_sent);
    }

    #[test]
    fn input_during_running_queues_next_turn() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.finished = false;
        pane.turn_running = true;

        for ch in "j/kも入力".chars() {
            pane.input(key(KeyCode::Char(ch)));
        }
        pane.input(key(KeyCode::Enter));

        assert_eq!(pane.queued_prompt_count(), 1);
        assert_eq!(pane.input_line(), "");
        assert_eq!(pane.last_prompt(), Some("j/kも入力"));
        assert!(
            pane.log
                .iter()
                .any(|line| line.kind == CodexLogKind::User && line.text == "j/kも入力")
        );
        assert!(pane.log.iter().any(
            |line| line.kind == CodexLogKind::System && line.text.contains("次のターンに予約")
        ));
    }

    #[test]
    fn queued_exit_finishes_when_turn_becomes_idle() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.finished = false;
        pane.turn_running = true;

        for ch in "/exit".chars() {
            pane.input(key(KeyCode::Char(ch)));
        }
        pane.input(key(KeyCode::Enter));

        assert!(!pane.finished);
        assert_eq!(pane.queued_prompt_count(), 1);

        pane.turn_running = false;
        assert!(pane.start_next_queued_turn_if_idle());

        assert!(pane.finished);
        assert_eq!(pane.queued_prompt_count(), 0);
    }

    #[test]
    fn scrollback_clamps_to_log_history() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "1\n2\n3\n4\n5\n6\n7\n8\n9\n10");
        pane.resize(6, 20);

        pane.scroll_lines(99);

        assert_eq!(pane.scrollback, pane.max_view_scrollback());
    }

    #[test]
    fn rendered_history_metrics_define_exact_scrollback_limit() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");

        pane.sync_rendered_history_metrics(100, 10);
        pane.scroll_to_top();
        assert_eq!(pane.scrollback, 90);

        pane.sync_rendered_history_metrics(12, 10);
        assert_eq!(pane.scrollback, 2);
    }

    #[test]
    fn thread_started_event_records_thread_id() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(r#"{"type":"thread.started","thread_id":"abc"}"#);

        assert_eq!(pane.thread_id(), Some("abc"));
        assert!(pane.log.iter().any(|line| line.text.contains("abc")));
    }

    #[test]
    fn turn_started_event_records_turn_separator() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.input_state.last_prompt = Some("  実装を進めて\nください  ".to_string());

        pane.handle_stdout_line(r#"{"type":"turn.started"}"#);

        let line = pane.log.last().unwrap();
        assert_eq!(line.kind, CodexLogKind::Turn);
        assert_eq!(line.text, "Turn 1 - 実装を進めて ください");
        assert!(pane.is_turn_running());
    }

    #[test]
    fn run_state_tracks_command_and_confirmation() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        assert_eq!(pane.run_state(), CodexRunState::Completed);
        pane.finished = false;
        assert_eq!(pane.run_state(), CodexRunState::InputWaiting);

        pane.handle_stdout_line(r#"{"type":"turn.started"}"#);
        assert_eq!(pane.run_state(), CodexRunState::Thinking);

        pane.handle_stdout_line(
            r#"{"type":"exec_command_begin","parsed_cmd":"cargo test","codex_cwd":"/repo"}"#,
        );
        assert_eq!(pane.run_state(), CodexRunState::CommandRunning);
        assert_eq!(pane.current_command(), Some("cargo test"));

        pane.handle_stdout_line(r#"{"type":"approval_requested","message":"Run command? y/n"}"#);
        let banner = pane.decision_banner().unwrap();
        assert_eq!(pane.run_state(), CodexRunState::Confirming);
        assert_eq!(banner.kind, CodexDecisionKind::Approval);

        assert!(pane.handle_decision_key(key(KeyCode::Char('a'))));
        assert!(pane.decision_banner().is_none());
        assert_eq!(pane.run_state(), CodexRunState::CommandRunning);
    }

    #[test]
    fn old_turns_are_collapsed_until_toggled_open() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.handle_stdout_line(r#"{"type":"turn.started"}"#);
        pane.push_log(CodexLogKind::Assistant, "old response");
        pane.handle_stdout_line(r#"{"type":"turn.completed"}"#);
        pane.handle_stdout_line(r#"{"type":"turn.started"}"#);
        pane.push_log(CodexLogKind::Assistant, "new response");

        let collapsed = pane
            .filtered_log_lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();
        assert!(collapsed.contains(&"Turn 1"));
        assert!(!collapsed.contains(&"old response"));
        assert!(collapsed.contains(&"new response"));

        pane.toggle_old_turns_collapsed();
        let expanded = pane
            .filtered_log_lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();
        assert!(expanded.contains(&"old response"));
    }

    #[test]
    fn filtered_log_lines_supports_filter_and_search() {
        let mut pane = CodexPane::test_with_output(8, 40, 0, "");
        pane.push_log(CodexLogKind::Turn, "Turn 1");
        pane.push_log(CodexLogKind::User, "cargo test して");
        pane.push_log(CodexLogKind::Assistant, "確認します");
        pane.push_log(CodexLogKind::Tool, "exec_command_begin: cargo test");
        pane.push_log(CodexLogKind::Error, "limit");

        pane.cycle_log_filter();
        let conversation = pane.filtered_log_lines();
        assert_eq!(conversation.len(), 3);
        assert!(conversation.iter().all(|line| matches!(
            line.kind,
            CodexLogKind::Turn | CodexLogKind::User | CodexLogKind::Assistant
        )));

        pane.cycle_log_filter();
        pane.search_query = "cargo".to_string();
        let tools = pane.filtered_log_lines();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].kind, CodexLogKind::Tool);
    }

    #[test]
    fn error_event_sets_terminal_notice() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(r#"{"type":"error","message":"limit"}"#);

        let notice = pane.take_terminal_notice().unwrap();
        assert_eq!(notice.title, "Codex エラー");
        assert_eq!(notice.message, "limit");
    }

    #[test]
    fn turn_completed_sets_terminal_notice() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(r#"{"type":"turn.completed"}"#);

        let notice = pane.take_terminal_notice().unwrap();
        assert_eq!(notice.title, "Codex 完了");
        assert_eq!(notice.message, "Codex の出力が完了しました");
    }

    #[test]
    fn approval_event_sets_terminal_notice_without_overwriting_next_notice() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(r#"{"type":"approval_requested","message":"Run command? yes/no"}"#);
        pane.handle_stdout_line(r#"{"type":"turn.completed"}"#);

        let first = pane.take_terminal_notice().unwrap();
        let second = pane.take_terminal_notice().unwrap();
        assert_eq!(first.title, "Codex 確認待ち");
        assert!(first.message.contains("yes/no"));
        assert_eq!(second.title, "Codex 完了");
    }

    #[test]
    fn assistant_delta_appends_to_same_line() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(r#"{"type":"agent_message.delta","delta":"hel"}"#);
        pane.handle_stdout_line(r#"{"type":"agent_message.delta","delta":"lo"}"#);

        let assistant = pane
            .log
            .iter()
            .filter(|line| line.kind == CodexLogKind::Assistant)
            .collect::<Vec<_>>();
        assert_eq!(assistant.len(), 1);
        assert_eq!(assistant[0].text, "hello");
    }

    #[test]
    fn session_history_persists_raw_events_and_restores_display_log() {
        let path = std::env::temp_dir().join(format!(
            "addness-codex-history-{}-{}.jsonl",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let _ = std::fs::remove_file(&path);
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        {
            let history_path = path.clone();
            let mut pane = CodexPane::spawn_inner(CodexPaneSpawnOptions {
                codex_bin: Path::new("codex"),
                cwd: &cwd,
                addness_bin: "addness",
                goal_id: "goal/history".to_string(),
                goal_title: "History goal".to_string(),
                dod: String::new(),
                status_label: "TEST".to_string(),
                session_log_path: Some(history_path),
            })
            .unwrap();
            pane.handle_stdout_line(r#"{"type":"agent_message.delta","delta":"hel"}"#);
            pane.handle_stdout_line(r#"{"type":"agent_message.delta","delta":"lo"}"#);
        }

        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains(r#""record":"raw_event""#));
        assert!(saved.contains(r#""record":"assistant_delta""#));

        let history_path = path.clone();
        let pane = CodexPane::spawn_inner(CodexPaneSpawnOptions {
            codex_bin: Path::new("codex"),
            cwd: &cwd,
            addness_bin: "addness",
            goal_id: "goal/history".to_string(),
            goal_title: "History goal".to_string(),
            dod: String::new(),
            status_label: "TEST".to_string(),
            session_log_path: Some(history_path),
        })
        .unwrap();

        assert!(pane.loaded_history_count() > 0);
        assert!(
            pane.log
                .iter()
                .any(|line| line.kind == CodexLogKind::Assistant && line.text == "hello")
        );
        assert!(pane.history_label().contains("保存中"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn response_item_exec_command_arguments_json_renders_tool_line() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"cargo test\",\"workdir\":\"/repo\"}"}}"#,
        );

        let line = pane.log.last().unwrap();
        assert_eq!(line.kind, CodexLogKind::Tool);
        assert!(line.text.contains("exec_command"));
        assert!(line.text.contains("cargo test"));
        assert!(line.text.contains("/repo"));
    }

    #[test]
    fn addness_bin_exec_command_updates_action_indicator() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"\\\"$ADDNESS_BIN\\\" goal get goal-1 --json\",\"workdir\":\"/repo\"}"}}"#,
        );

        assert_eq!(pane.action.as_deref(), Some("ゴール文脈を読込中"));
        assert!(pane.last_addness_read_at.is_some());
    }

    #[test]
    fn exec_command_begin_renders_command_from_native_fields() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"exec_command_begin","parsed_cmd":"cargo check","codex_cwd":"/repo"}"#,
        );

        let line = pane.log.last().unwrap();
        assert_eq!(line.kind, CodexLogKind::Tool);
        assert!(line.text.contains("exec_command_begin"));
        assert!(line.text.contains("cargo check"));
        assert!(line.text.contains("/repo"));
        assert_eq!(pane.current_command(), Some("cargo check"));
    }

    #[test]
    fn apply_patch_renders_as_code_edit_tool() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"response_item","payload":{"type":"function_call","name":"apply_patch","arguments":"*** Begin Patch\n*** Update File: src/tui/ui.rs\n@@\n*** End Patch\n"}}"#,
        );

        let line = pane.log.last().unwrap();
        assert_eq!(line.kind, CodexLogKind::Tool);
        assert!(line.text.starts_with("EDIT "));
        assert!(line.text.contains("*** Update File: src/tui/ui.rs"));
    }

    #[test]
    fn exec_command_end_renders_exit_code_and_output() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"exec_command_end","cmd":"cargo check","exit_code":0,"formatted_output":"Finished dev profile"}"#,
        );

        let line = pane.log.last().unwrap();
        assert_eq!(line.kind, CodexLogKind::Tool);
        assert!(line.text.contains("exec_command_end"));
        assert!(line.text.contains("exit 0"));
        assert!(line.text.contains("cargo check"));
        assert!(line.text.contains("Finished dev profile"));
        assert_eq!(pane.current_command(), None);
    }

    #[test]
    fn turn_completed_clears_current_command() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"exec_command_begin","parsed_cmd":"cargo check","codex_cwd":"/repo"}"#,
        );
        assert_eq!(pane.current_command(), Some("cargo check"));

        pane.handle_stdout_line(r#"{"type":"turn.completed"}"#);

        assert_eq!(pane.current_command(), None);
    }

    #[test]
    fn addness_tool_activity_summarizes_semantic_change() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"$ADDNESS_BIN goal update goal-1 --body memo --json\",\"workdir\":\"/repo\"}"},"formatted_output":"{\"title\":\"重要ゴール\"}"}"#,
        );

        assert!(pane.last_addness_write_at.is_some());
        assert!(
            pane.activity
                .iter()
                .any(|line| line.contains("Addness body変更: 重要ゴール"))
        );
    }

    #[test]
    fn work_summary_extracts_assistant_checks_and_errors() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.push_log(
            CodexLogKind::Assistant,
            "UIを改善しました\nテストを追加しました",
        );
        pane.push_log(
            CodexLogKind::Tool,
            "OK cargo test (exit 0)\ntest result: ok. 99 passed; 0 failed;",
        );
        pane.push_log(CodexLogKind::Error, "残課題: なし");

        let summary = pane.work_summary();
        assert!(
            summary
                .implemented
                .iter()
                .any(|line| line.contains("UIを改善"))
        );
        assert!(
            summary
                .checks
                .iter()
                .any(|line| line.contains("test result: ok"))
        );
        assert!(summary.remaining.iter().any(|line| line.contains("残課題")));
    }

    #[test]
    fn agent_message_text_is_not_classified_as_tool() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"agent_message","message":"I would run cargo test next."}"#,
        );

        let line = pane.log.last().unwrap();
        assert_eq!(line.kind, CodexLogKind::Assistant);
        assert_eq!(line.text, "I would run cargo test next.");
    }

    #[test]
    fn codex_exec_args_start_new_json_turn() {
        let args = codex_exec_args(None, "/repo");

        assert_eq!(args[0], "exec");
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
    }

    #[test]
    fn codex_exec_args_resume_existing_json_thread() {
        let args = codex_exec_args(Some("thread-1"), "/repo");

        assert_eq!(args[0], "exec");
        assert_eq!(args[1], "resume");
        assert!(args.contains(&"--json".to_string()));
        assert!(args.contains(&"thread-1".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("-"));
        assert!(!args.contains(&"-C".to_string()));
        assert!(!args.contains(&"-s".to_string()));
    }

    #[test]
    fn dod_prompt_lists_items() {
        let prompt = build_dod_assessment_prompt(&["A".to_string(), "B".to_string()]);

        assert!(prompt.contains("0: A"));
        assert!(prompt.contains("1: B"));
    }
}
