use anyhow::Result;
use chrono::{Local, Utc};
use ratatui::{
    DefaultTerminal,
    crossterm::event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
    },
    layout::Rect,
};
use std::{
    collections::HashMap,
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use crate::api::{
    ApiClient, Comment, CreateGoalRequest, Deliverable, DeliverableType, GoalChildItem, GoalStatus,
    Member, MemberId, Organization, UpdateGoalRequest,
};
use crate::dbg_log;

use super::agent::{self, AgentKind, ChildGoalUpdate, CodexPane, CodexWorkSummary, TerminalNotice};
use super::codex_memory::{codex_body_update_request, codex_trace_link_label, codex_work_memo};
pub(super) use super::file_picker::PICKER_VISIBLE_ROWS;
use super::file_picker::{
    FileEntry, FilePickerReturn, complete_path, initial_picker_dir, read_dir_entries,
};
use super::goal_tree::{GoalTree, TreeRow};
use super::ui;

#[derive(PartialEq, Eq)]
pub enum ActivePane {
    OrgSelector,
    Navigation,
    Content,
    /// 埋め込み codex ペインにフォーカス中。キー入力は codex へ転送される。
    Codex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormField {
    Title,
    Description,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliverableKind {
    File,
    Document,
    Link,
    Folder,
}

impl DeliverableKind {
    fn next(&self) -> Self {
        match self {
            DeliverableKind::File => DeliverableKind::Document,
            DeliverableKind::Document => DeliverableKind::Link,
            DeliverableKind::Link => DeliverableKind::Folder,
            DeliverableKind::Folder => DeliverableKind::File,
        }
    }

    fn prev(&self) -> Self {
        match self {
            DeliverableKind::File => DeliverableKind::Folder,
            DeliverableKind::Document => DeliverableKind::File,
            DeliverableKind::Link => DeliverableKind::Document,
            DeliverableKind::Folder => DeliverableKind::Link,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            DeliverableKind::File => "file",
            DeliverableKind::Document => "document",
            DeliverableKind::Link => "link",
            DeliverableKind::Folder => "folder",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliverableFormField {
    Kind,
    Name,
    Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionMenuItem {
    WorkWithCodex,
    WorkWithClaude,
    AddDeliverable,
    AddComment,
    CompleteGoal,
    ReopenGoal,
    EditGoal,
    DeleteGoal,
    UpdateDeliverable,
    RenameDeliverable,
    MoveDeliverable,
    DeleteDeliverable,
    ReplyComment,
    ResolveComment,
    UnresolveComment,
    EditComment,
    DeleteComment,
    ReactComment,
}

impl ActionMenuItem {
    pub fn label(&self) -> &'static str {
        match self {
            ActionMenuItem::WorkWithCodex => "codexで作業",
            ActionMenuItem::WorkWithClaude => "claude codeで作業",
            ActionMenuItem::AddDeliverable => "add deliverable",
            ActionMenuItem::AddComment => "add comment",
            ActionMenuItem::CompleteGoal => "complete goal",
            ActionMenuItem::ReopenGoal => "reopen goal",
            ActionMenuItem::EditGoal => "edit goal",
            ActionMenuItem::DeleteGoal => "delete goal",
            ActionMenuItem::UpdateDeliverable => "update document",
            ActionMenuItem::RenameDeliverable => "rename deliverable",
            ActionMenuItem::MoveDeliverable => "move deliverable",
            ActionMenuItem::DeleteDeliverable => "delete deliverable",
            ActionMenuItem::ReplyComment => "reply to comment",
            ActionMenuItem::ResolveComment => "resolve comment",
            ActionMenuItem::UnresolveComment => "unresolve comment",
            ActionMenuItem::EditComment => "edit comment",
            ActionMenuItem::DeleteComment => "delete comment",
            ActionMenuItem::ReactComment => "react",
        }
    }
}

/// Display status combining GoalStatus and completed_at
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalDisplayStatus {
    NotStarted, // GoalStatus::None + not completed
    InProgress, // GoalStatus::InProgress + not completed
    Cancelled,  // GoalStatus::Cancelled + not completed
    Completed,  // completed_at.is_some()
}

impl GoalDisplayStatus {
    /// Get allowed transitions from current status
    pub fn allowed_transitions(&self) -> Vec<GoalDisplayStatus> {
        match self {
            GoalDisplayStatus::NotStarted => vec![
                GoalDisplayStatus::InProgress,
                GoalDisplayStatus::Cancelled,
                GoalDisplayStatus::Completed,
            ],
            GoalDisplayStatus::InProgress => vec![
                GoalDisplayStatus::NotStarted,
                GoalDisplayStatus::Cancelled,
                GoalDisplayStatus::Completed,
            ],
            GoalDisplayStatus::Cancelled => vec![
                GoalDisplayStatus::NotStarted,
                GoalDisplayStatus::InProgress,
                GoalDisplayStatus::Completed,
            ],
            GoalDisplayStatus::Completed => vec![
                // Completed is final state - no transitions allowed
            ],
        }
    }

    pub fn to_emoji_string(&self) -> String {
        match self {
            GoalDisplayStatus::NotStarted => "未着手".to_string(),
            GoalDisplayStatus::InProgress => "進行中".to_string(),
            GoalDisplayStatus::Cancelled => "停止中".to_string(),
            GoalDisplayStatus::Completed => "完了".to_string(),
        }
    }

    /// 状態を表す短いマーカー（子ゴールリストのアイコン用、ASCII）。
    pub fn icon(&self) -> &'static str {
        match self {
            GoalDisplayStatus::NotStarted => "[ ]",
            GoalDisplayStatus::InProgress => "[~]",
            GoalDisplayStatus::Cancelled => "[-]",
            GoalDisplayStatus::Completed => "[x]",
        }
    }

    /// Convert to GoalStatus for API (Completed is handled separately via completed_at)
    pub fn to_goal_status(&self) -> GoalStatus {
        match self {
            GoalDisplayStatus::NotStarted => GoalStatus::None,
            GoalDisplayStatus::InProgress => GoalStatus::InProgress,
            GoalDisplayStatus::Cancelled => GoalStatus::Cancelled,
            GoalDisplayStatus::Completed => GoalStatus::None, // Will set completed_at instead
        }
    }

    /// Create from GoalStatus and is_completed flag
    pub fn from_goal_state(status: Option<&GoalStatus>, is_completed: bool) -> Self {
        if is_completed {
            return GoalDisplayStatus::Completed;
        }
        match status {
            Some(GoalStatus::InProgress) => GoalDisplayStatus::InProgress,
            Some(GoalStatus::Cancelled) => GoalDisplayStatus::Cancelled,
            _ => GoalDisplayStatus::NotStarted,
        }
    }
}

fn child_goal_update_from_item(child: GoalChildItem) -> ChildGoalUpdate {
    let status = GoalDisplayStatus::from_goal_state(child.status.as_ref(), child.is_completed);
    ChildGoalUpdate {
        id: child.id,
        title: child.title,
        description: child.description,
        icon: status.icon(),
        status_label: status.to_emoji_string(),
        is_completed: status == GoalDisplayStatus::Completed,
    }
}

/// コメント本文を一覧表示・タイトル用に短く切り詰める（改行は空白化）。
fn truncate_comment(content: &str) -> String {
    let oneline = content.replace(['\n', '\r'], " ");
    let trimmed = oneline.trim();
    let max = 40;
    if trimmed.chars().count() > max {
        let head: String = trimmed.chars().take(max).collect();
        format!("{head}…")
    } else {
        trimmed.to_string()
    }
}

// NOTE: 将来goal以外についてのモーダルが出得るためsuffixにGoalと付けている
// それまでclippy errorを抑制
#[allow(clippy::enum_variant_names)]
pub enum ModalState {
    ActionMenu {
        title: String,
        items: Vec<ActionMenuItem>,
        selected_index: usize,
    },
    CreateGoal {
        title: String,
        description: String,
        parent_goal_id: Option<String>,
        parent_goal_title: Option<String>,
        current_field: FormField,
    },
    EditGoal {
        goal_id: String,
        title: String,
        description: String,
        current_status: GoalDisplayStatus,
        selected_status_index: usize,
        allowed_statuses: Vec<GoalDisplayStatus>,
        current_field: FormField,
    },
    DeleteGoal {
        goal_id: String,
        goal_title: String,
        confirm_index: usize, // 0 = Cancel (default), 1 = Delete
    },
    AddDeliverable {
        goal_id: String,
        goal_title: String,
        kind: DeliverableKind,
        name: String,
        value: String,
        current_field: DeliverableFormField,
    },
    UpdateDeliverable {
        goal_id: String,
        deliverable_id: String,
        deliverable_name: String,
        content_file: String,
    },
    RenameDeliverable {
        goal_id: String,
        deliverable_id: String,
        current_name: String,
        name: String,
    },
    MoveDeliverable {
        goal_id: String,
        deliverable_id: String,
        deliverable_name: String,
        targets: Vec<(Option<String>, String)>,
        selected_index: usize,
    },
    DeleteDeliverable {
        goal_id: String,
        deliverable_id: String,
        deliverable_name: String,
        confirm_index: usize,
    },
    AddComment {
        goal_id: String,
        goal_title: String,
        body: String,
    },
    ReplyComment {
        goal_id: String,
        parent_comment_id: String,
        parent_excerpt: String,
        body: String,
    },
    EditComment {
        goal_id: String,
        comment_id: String,
        body: String,
    },
    DeleteComment {
        goal_id: String,
        comment_id: String,
        excerpt: String,
        confirm_index: usize,
    },
    ReactComment {
        goal_id: String,
        comment_id: String,
        emojis: Vec<&'static str>,
        selected_index: usize,
    },
    FilePicker {
        dir: PathBuf,
        entries: Vec<FileEntry>,
        selected_index: usize,
        ret: FilePickerReturn,
    },
}

const CONFIRM_CANCEL: usize = 0;
const CONFIRM_APPLY: usize = 1;
const CONFIRM_ALWAYS: usize = 2;
const CONFIRM_CHOICE_COUNT: usize = 3;

/// 起動時にバックグラウンドで取得する初期データ。
/// `&mut self` を奪わずに別タスクで取得できるよう、所有データだけを持つ。
struct InitialData {
    orgs: Vec<Organization>,
    goal_tree: GoalTree,
    /// 取得中に起きた最初のエラー（ステータスバーに表示する）。
    error: Option<String>,
}

struct RootGoalDetail {
    goal_id: String,
    comments: Vec<Comment>,
    deliverables: Vec<Deliverable>,
}

#[derive(Default)]
struct RootGoalDetails {
    items: Vec<RootGoalDetail>,
    failed: usize,
}

struct DeferredInitialData {
    members_list: Vec<Member>,
    root_details: RootGoalDetails,
    error: Option<String>,
}

fn root_goal_ids(tree: &GoalTree) -> Vec<String> {
    tree.flatten()
        .iter()
        .filter_map(|row| match row {
            TreeRow::Goal { goal_id, depth, .. } if *depth == 0 => Some(goal_id.to_string()),
            _ => None,
        })
        .collect()
}

/// ルートゴール（depth==0）のコメント・成果物を並列取得する。
/// 専用 helper(get_*_map) は失敗を stderr に出力して握り潰すため、TUI では使わない。
async fn fetch_root_goal_details(client: &ApiClient, root_ids: Vec<String>) -> RootGoalDetails {
    if root_ids.is_empty() {
        return RootGoalDetails::default();
    }

    // 逐次 N 往復 → 概ね 1 往復に畳む。
    let fetched = futures::future::join_all(root_ids.into_iter().map(|id| async move {
        let r = tokio::try_join!(client.list_comments(&id), client.get_goal_deliverables(&id));
        (id, r)
    }))
    .await;

    let mut failed = 0usize;
    let mut items = Vec::new();
    for (id, r) in fetched {
        match r {
            Ok((comments, deliverables)) => {
                items.push(RootGoalDetail {
                    goal_id: id,
                    comments: comments.comments,
                    deliverables: deliverables.data.deliverables,
                });
            }
            Err(_) => failed += 1,
        }
    }
    RootGoalDetails { items, failed }
}

fn apply_root_goal_details(tree: &mut GoalTree, details: RootGoalDetails) -> usize {
    let failed = details.failed;
    for detail in details.items {
        tree.set_comments_for_goal_id(&detail.goal_id, detail.comments);
        tree.set_deliverables_for_goal_id(&detail.goal_id, detail.deliverables);
    }
    failed
}

/// 組織・ゴールツリーを取得する。
/// `ApiClient` のクローンを所有し、`App` を借用しないため `spawn` できる。
async fn fetch_initial_data(mut client: ApiClient) -> InitialData {
    let orgs = match client.list_organizations().await {
        Ok(resp) => resp.data.organizations,
        Err(e) => {
            return InitialData {
                orgs: Vec::new(),
                goal_tree: GoalTree::empty(),
                error: Some(format!("Failed to load organizations: {e}")),
            };
        }
    };

    let Some(org_id) = orgs.first().map(|o| o.id.clone()) else {
        return InitialData {
            orgs,
            goal_tree: GoalTree::empty(),
            error: None,
        };
    };
    client.set_org_id(Some(org_id.clone()));

    let (goal_tree, error) = match client.get_goal_tree(&org_id, 2).await {
        Ok(resp) => (GoalTree::from_tree_items(resp.data.items), None),
        Err(e) => (
            GoalTree::empty(),
            Some(format!("Failed to load goals: {e}")),
        ),
    };

    InitialData {
        orgs,
        goal_tree,
        error,
    }
}

async fn fetch_deferred_initial_data(
    mut client: ApiClient,
    org_id: String,
    root_ids: Vec<String>,
) -> DeferredInitialData {
    client.set_org_id(Some(org_id.clone()));
    let (members_res, root_details) = tokio::join!(
        client.get_members(&org_id),
        fetch_root_goal_details(&client, root_ids)
    );
    let (members_list, error) = match members_res {
        Ok(resp) => (resp.data.members, None),
        Err(e) => (Vec::new(), Some(format!("Failed to load members: {e}"))),
    };
    DeferredInitialData {
        members_list,
        root_details,
        error,
    }
}

/// codex exec が出力した JSON 文字列から DoD 判定結果を取り出す。
/// `{ "results": [ { "index": <int>, "met": <bool> } ] }` を期待する。
fn parse_dod_results(content: &str, item_count: usize) -> Option<Vec<(usize, bool)>> {
    let json_str = extract_json_object(content)?;
    let value: serde_json::Value = serde_json::from_str(&json_str).ok()?;
    let arr = value.get("results")?.as_array()?;
    let mut out = Vec::new();
    for item in arr {
        // 1 件が壊れていても（負の index・型違い等）全体を捨てず、その項目だけ飛ばす。
        let (Some(idx), Some(met)) = (
            item.get("index").and_then(|v| v.as_u64()),
            item.get("met").and_then(|v| v.as_bool()),
        ) else {
            continue;
        };
        let idx = idx as usize;
        if idx < item_count {
            out.push((idx, met));
        }
    }
    Some(out)
}

/// 文字列中の最初の `{` から最後の `}` までを JSON オブジェクトとして切り出す。
/// codex の最終メッセージに余計な前後テキストが付いても拾えるようにする。
fn extract_json_object(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    (end > start).then(|| s[start..=end].to_string())
}

/// DoD 自動判定がハングした場合に強制終了するまでの上限時間。
const DOD_ASSESSMENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

const CODEX_WHEEL_LINES: isize = 6;
const CODEX_MOUSE_DRAIN_LIMIT: usize = 64;
const XTERM_MOUSE_CAPTURE_ON: &[u8] = b"\x1b[?1000h\x1b[?1006h\x1b[?1007h";
const XTERM_MOUSE_CAPTURE_OFF: &[u8] =
    b"\x1b[?1007l\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexTerminalScrollRoute {
    Scrollback,
    None,
}

fn is_permission_denied_error_text(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("403")
        || lower.contains("forbidden")
        || lower.contains("permission")
        || lower.contains("objective.update")
        || text.contains("権限")
}

/// codex ペインのライブ更新で取得する Addness 側のスナップショット。
struct CodexSnapshot {
    title: String,
    description: String,
    body: Option<String>,
    status_label: String,
    comment_count: Option<usize>,
    deliverable_count: Option<usize>,
    trace_links: Vec<String>,
    /// 子ゴール一覧。None=取得失敗。
    children: Option<Vec<ChildGoalUpdate>>,
}

/// 実行中の DoD 自動判定（codex exec）ジョブ。
struct DodJob {
    child: std::process::Child,
    /// codex に渡した出力先と一時スキーマファイル（完了時に掃除する）。
    out_path: PathBuf,
    schema_path: PathBuf,
    item_count: usize,
    /// 起動時刻。タイムアウト判定に使う。
    started: Instant,
}

struct CodexBodyRecordOutcome {
    ok: bool,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexBodyRecordResult {
    Written,
    SkippedPermission,
    Failed,
}

impl DodJob {
    /// 子プロセスを強制終了し（ゾンビ化を防ぐため wait し）、一時ファイルを掃除する。
    fn cleanup(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.out_path);
        let _ = std::fs::remove_file(&self.schema_path);
    }
}

fn emit_terminal_notification(notice: &TerminalNotice) {
    let title = terminal_notification_text(&notice.title, 80);
    let message = terminal_notification_text(&notice.message, 240);
    if message.is_empty() {
        return;
    }

    let mut stdout = std::io::stdout();
    let _ = write!(
        stdout,
        "\x07\x1b]9;{message}\x07\x1b]777;notify;{title};{message}\x07"
    );
    let _ = stdout.flush();
}

fn terminal_notification_text(input: &str, max_chars: usize) -> String {
    let collapsed = input
        .chars()
        .filter_map(|ch| match ch {
            '\n' | '\r' | '\t' => Some(' '),
            ';' => Some(' '),
            ch if ch.is_control() => None,
            ch => Some(ch),
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }

    let keep = max_chars.saturating_sub(3);
    let mut out = collapsed.chars().take(keep).collect::<String>();
    out.push_str("...");
    out
}

pub struct App {
    pub(super) client: ApiClient,
    rt: Handle,
    pub running: bool,
    pub active_pane: ActivePane,
    pub sidebar_index: usize,
    pub sidebar_items: Vec<&'static str>,

    // Organization state
    pub orgs: Vec<Organization>,
    pub current_org_index: usize,
    pub show_org_popup: bool,
    pub org_popup_index: usize,

    /// キーバインド一覧のヘルプオーバーレイ表示中か（`?` で開く）
    pub show_help: bool,
    /// ヘルプオーバーレイのスクロール位置（行数）。描画側が実際の行数に
    /// クランプし直す。開くたびに 0 へ戻す。
    pub help_scroll: usize,

    // Goal trees
    pub goal_tree: GoalTree,
    pub todays_goals_tree: GoalTree,
    /// Execution(todays) ツリーは初回表示時に遅延ロードする。起動を軽くするため。
    todays_loaded: bool,

    // Members state
    pub members: HashMap<MemberId, Member>,
    pub members_list: Vec<Member>, // For display in Members tab
    pub members_cursor: usize,
    pub members_scroll_offset: usize,

    /// Last known content viewport height (for scroll calculations)
    pub content_height: usize,

    /// Error message to display in status bar (cleared on next key press)
    pub error_message: Option<String>,

    /// Modal state for create/edit goal dialogs
    pub modal_state: Option<ModalState>,
    /// このTUIセッション中、同種の削除確認を省略する。
    allow_delete_goal_without_confirm: bool,
    allow_delete_deliverable_without_confirm: bool,
    allow_delete_comment_without_confirm: bool,

    /// Success message to display in status bar (cleared on next key press)
    pub success_message: Option<String>,

    /// 埋め込み codex セッション（起動中のみ Some）
    pub codex: Option<CodexPane>,
    /// 直近に描画した codex 端末ペインの外枠領域。マウス座標のローカル変換に使う。
    pub(super) codex_terminal_area: Option<Rect>,
    /// 直近に描画した Codex 左ペインのスクロール対象領域。
    pub(super) codex_status_area: Option<Rect>,
    pub(super) codex_contract_area: Option<Rect>,
    pub(super) codex_activity_area: Option<Rect>,
    pub(super) codex_contract_scroll: usize,
    pub(super) codex_activity_scroll: usize,
    pub(super) codex_last_scroll_input: Option<String>,

    /// codex 実行中、対象ゴールを低頻度・非ブロッキングで再取得するための
    /// バックグラウンドタスク（進行中のみ Some）と、前回リフレッシュ時刻。
    codex_refresh: Option<JoinHandle<Option<CodexSnapshot>>>,
    last_codex_refresh: Option<Instant>,
    /// 直近に Addness 同期が完了した時刻と回数（左ペインの鼓動表示に使う）。
    pub(super) last_codex_sync: Option<Instant>,
    pub(super) codex_sync_tick: u64,
    /// codex を閉じた直後、UI を即切替してからツリー再読込を遅延実行するためのフラグ。
    pending_codex_tree_reload: bool,
    /// 次の描画前に画面を全クリアする（codex ⇄ 通常UI の構造遷移やリサイズで
    /// 前画面の残像が残らないようにするため）。
    needs_full_clear: bool,

    /// DoD 自動判定（codex exec）の実行中ジョブ。
    codex_dod_job: Option<DodJob>,
    /// 最初の実依頼など、節目の Codex 作業メモを body に非同期記録するジョブ。
    codex_body_record_job: Option<JoinHandle<CodexBodyRecordOutcome>>,
    /// 初回表示後にメンバー・ルートゴール詳細を埋める遅延ロードジョブ。
    deferred_initial_load: Option<JoinHandle<DeferredInitialData>>,

    /// ステータスメッセージ（error/success）の自動クリア期限。
    /// キー入力では消さず、表示から一定時間で期限切れとしてクリアする。
    status_deadline: Option<Instant>,
    /// 直近に自動クリア期限を張ったときのメッセージ内容。新しいメッセージが
    /// 来たら期限を張り直すための比較用スナップショット。
    status_snapshot: (Option<String>, Option<String>),
}

impl App {
    pub fn new(client: ApiClient, rt: Handle) -> Self {
        Self {
            client,
            rt,
            running: true,
            active_pane: ActivePane::Navigation,
            sidebar_index: 0,
            sidebar_items: vec!["Goals", "Execution", "Members"],
            orgs: vec![],
            current_org_index: 0,
            show_org_popup: false,
            org_popup_index: 0,
            show_help: false,
            help_scroll: 0,
            goal_tree: GoalTree::empty(),
            todays_goals_tree: GoalTree::empty(),
            todays_loaded: false,
            members: HashMap::new(),
            members_list: vec![],
            members_cursor: 0,
            members_scroll_offset: 0,
            content_height: 0,
            error_message: None,
            modal_state: None,
            allow_delete_goal_without_confirm: false,
            allow_delete_deliverable_without_confirm: false,
            allow_delete_comment_without_confirm: false,
            success_message: None,
            codex: None,
            codex_terminal_area: None,
            codex_status_area: None,
            codex_contract_area: None,
            codex_activity_area: None,
            codex_contract_scroll: 0,
            codex_activity_scroll: 0,
            codex_last_scroll_input: None,
            codex_refresh: None,
            last_codex_refresh: None,
            last_codex_sync: None,
            codex_sync_tick: 0,
            pending_codex_tree_reload: false,
            needs_full_clear: false,
            codex_dod_job: None,
            codex_body_record_job: None,
            deferred_initial_load: None,
            status_deadline: None,
            status_snapshot: (None, None),
        }
    }

    /// ステータスメッセージ（error/success）の自動クリアTTL。
    const STATUS_MESSAGE_TTL: Duration = Duration::from_secs(5);

    /// 表示中のメッセージが前回から変化していれば、自動クリア期限を張り直す。
    /// 88箇所ある `error_message = Some(..)` 等の代入を毎回置き換えず、ループ側で
    /// 内容の変化を検知して一元的に期限を管理する。
    fn refresh_status_deadline(&mut self) {
        if self.error_message == self.status_snapshot.0
            && self.success_message == self.status_snapshot.1
        {
            return;
        }
        self.status_snapshot = (self.error_message.clone(), self.success_message.clone());
        self.status_deadline = if self.error_message.is_some() || self.success_message.is_some() {
            Some(Instant::now() + Self::STATUS_MESSAGE_TTL)
        } else {
            None
        };
    }

    /// 自動クリア期限が過ぎていればメッセージを消す。消したら true を返す（要再描画）。
    fn expire_status_messages(&mut self) -> bool {
        match self.status_deadline {
            Some(deadline) if Instant::now() >= deadline => {
                self.error_message = None;
                self.success_message = None;
                self.status_deadline = None;
                self.status_snapshot = (None, None);
                true
            }
            _ => false,
        }
    }

    pub fn current_org_name(&self) -> &str {
        self.orgs
            .get(self.current_org_index)
            .map(|o| o.name.as_str())
            .unwrap_or("(no org)")
    }

    pub fn current_org_id(&self) -> Option<&str> {
        self.orgs.get(self.current_org_index).map(|o| o.id.as_str())
    }

    /// Get reference to the currently active goal tree based on sidebar selection
    pub fn active_goal_tree(&self) -> &GoalTree {
        match self.sidebar_index {
            0 => &self.goal_tree,
            1 => &self.todays_goals_tree,
            _ => &self.goal_tree,
        }
    }

    /// Get mutable reference to the currently active goal tree based on sidebar selection
    pub fn active_goal_tree_mut(&mut self) -> &mut GoalTree {
        match self.sidebar_index {
            0 => &mut self.goal_tree,
            1 => &mut self.todays_goals_tree,
            _ => &mut self.goal_tree,
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        // 初期データ取得をバックグラウンドタスクで走らせ、その間メインスレッドは
        // ロゴを波打たせるフレームを一定間隔で描き続ける（取得完了まで）。
        let mut handle = self.rt.spawn(fetch_initial_data(self.client.clone()));
        terminal.draw(|frame| ui::draw_loading(frame, 0))?;
        let mut tick: u64 = 1;
        while !handle.is_finished() {
            std::thread::sleep(std::time::Duration::from_millis(80));
            terminal.draw(|frame| ui::draw_loading(frame, tick))?;
            tick = tick.wrapping_add(1);
        }
        let data = self
            .rt
            .block_on(&mut handle)
            .unwrap_or_else(|_| InitialData {
                orgs: Vec::new(),
                goal_tree: GoalTree::empty(),
                error: Some("Failed to load initial data".to_string()),
            });
        self.apply_initial_data(data);
        self.start_deferred_initial_load();

        // ロード画面（ロゴ）から本UIへ切り替わる初回フレームの残像を消す。
        self.needs_full_clear = true;
        let mut needs_redraw = true;
        while self.running {
            // 表示しようとしているタブのデータを必要になった時点で取得する。
            self.ensure_active_tab_loaded();
            if self.poll_deferred_initial_load() {
                needs_redraw = true;
            }
            if needs_redraw {
                // 構造遷移・リサイズ時は差分描画前に画面を全消去し、残像を断つ。
                if self.needs_full_clear {
                    terminal.clear()?;
                    self.needs_full_clear = false;
                }
                terminal.draw(|frame| ui::draw(frame, self))?;
                needs_redraw = false;
            }

            // codex を閉じた直後は、上の描画で UI を切り替えた後にツリーを再読込する。
            if self.pending_codex_tree_reload {
                self.pending_codex_tree_reload = false;
                self.load_goal_tree();
                if self.todays_loaded {
                    self.load_todays_goals();
                }
                needs_redraw = true;
                continue;
            }

            if self.codex.is_some() {
                // codex は非同期に描画更新するので、キー入力が無くても一定間隔で
                // JSONL 出力を取り込む（ブロッキング read は使わない）。変化があった
                // フレームだけ再描画し、アイドル時の無駄な再描画を避ける。
                if event::poll(Duration::from_millis(20))? {
                    self.handle_events()?;
                    needs_redraw = true;
                }
                if self.update_codex() {
                    needs_redraw = true;
                }
            } else {
                // ステータスメッセージは時間ベースで自動クリアするため、ブロッキング
                // read ではなく poll でタイムアウトさせ、時間経過を進める。メッセージ
                // 表示中のみ短い間隔でポーリングし、非表示（アイドル）時は長めにして
                // CPU 負荷を抑える。
                let timeout = if self.status_deadline.is_some() {
                    Duration::from_millis(100)
                } else {
                    Duration::from_millis(500)
                };
                if event::poll(timeout)? {
                    self.handle_events()?;
                    needs_redraw = true;
                }
            }

            // 表示中メッセージが変わっていれば期限を張り直し、期限切れなら消す。
            self.refresh_status_deadline();
            if self.expire_status_messages() {
                needs_redraw = true;
            }
        }
        Ok(())
    }

    /// バックグラウンドで取得した初期データを App の状態へ反映する。
    fn apply_initial_data(&mut self, data: InitialData) {
        self.orgs = data.orgs;
        if !self.orgs.is_empty() {
            self.current_org_index = 0;
        }
        // 後続の遅延ロードのため、選択中の組織IDをクライアントにも反映する。
        if let Some(org_id) = self.current_org_id().map(|s| s.to_string()) {
            self.client.set_org_id(Some(org_id));
        }
        self.goal_tree = data.goal_tree;
        if let Some(err) = data.error {
            self.error_message = Some(err);
        }
    }

    fn start_deferred_initial_load(&mut self) {
        if let Some(handle) = self.deferred_initial_load.take() {
            handle.abort();
        }
        let Some(org_id) = self.current_org_id().map(str::to_string) else {
            return;
        };
        let root_ids = root_goal_ids(&self.goal_tree);
        let client = self.client.clone();
        self.deferred_initial_load = Some(
            self.rt
                .spawn(fetch_deferred_initial_data(client, org_id, root_ids)),
        );
    }

    fn poll_deferred_initial_load(&mut self) -> bool {
        let Some(handle) = self.deferred_initial_load.as_ref() else {
            return false;
        };
        if !handle.is_finished() {
            return false;
        }
        let handle = self.deferred_initial_load.take().unwrap();
        let Ok(data) = self.rt.block_on(handle) else {
            self.error_message = Some("Failed to load goal details".to_string());
            return true;
        };
        self.set_members(data.members_list);
        let failed = apply_root_goal_details(&mut self.goal_tree, data.root_details);
        self.goal_tree.clamp_cursor();
        if failed > 0 {
            self.error_message = Some(format!("Failed to load details for {failed} goal(s)"));
        } else if let Some(err) = data.error {
            self.error_message = Some(err);
        }
        true
    }

    /// メンバー一覧をルックアップ用マップと表示用リストに反映し、カーソルを先頭へ戻す。
    fn set_members(&mut self, list: Vec<Member>) {
        self.members = list
            .iter()
            .map(|m| (MemberId::new(m.id.clone()), m.clone()))
            .collect();
        self.members_list = list;
        self.members_cursor = 0;
        self.members_scroll_offset = 0;
    }

    /// アクティブなタブが未ロードなら取得する（遅延ロード）。
    /// Execution(todays) は初回表示までロードしないことで起動を軽くする。
    fn ensure_active_tab_loaded(&mut self) {
        if self.sidebar_index == 1 && !self.todays_loaded {
            self.load_todays_goals();
        }
    }

    // -----------------------------------------------------------------------
    // API helpers
    // -----------------------------------------------------------------------

    fn api_call<F, T>(&self, future: F) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        self.rt.block_on(future)
    }

    /// 現在の組織配下のゴールツリーを取得し、重い詳細は遅延ロードへ逃がす。
    fn load_org_scoped_data(&mut self) {
        let Some(org_id) = self.current_org_id().map(|s| s.to_string()) else {
            self.goal_tree = GoalTree::empty();
            self.members = HashMap::new();
            self.members_list = vec![];
            return;
        };

        self.client.set_org_id(Some(org_id.clone()));

        match self.api_call(self.client.get_goal_tree(&org_id, 2)) {
            Ok(resp) => {
                self.goal_tree = GoalTree::from_tree_items(resp.data.items);
                self.members = HashMap::new();
                self.members_list = vec![];
                self.start_deferred_initial_load();
            }
            Err(e) => {
                self.goal_tree = GoalTree::empty();
                self.members = HashMap::new();
                self.members_list = vec![];
                self.error_message = Some(format!("Failed to load goals: {e}"));
            }
        }
    }

    fn load_goal_tree(&mut self) {
        let Some(org_id) = self.current_org_id().map(|s| s.to_string()) else {
            self.goal_tree = GoalTree::empty();
            return;
        };

        self.client.set_org_id(Some(org_id.clone()));

        match self.api_call(self.client.get_goal_tree(&org_id, 2)) {
            Ok(resp) => {
                self.goal_tree = GoalTree::from_tree_items(resp.data.items);
                self.start_deferred_initial_load();
            }
            Err(e) => {
                self.goal_tree = GoalTree::empty();
                self.error_message = Some(format!("Failed to load goals: {e}"));
            }
        }
    }

    fn load_todays_goals(&mut self) {
        // 一度試みたら（成否に関わらず）再取得しないようフラグを立てる。
        self.todays_loaded = true;
        let Some(org_id) = self.current_org_id().map(|s| s.to_string()) else {
            self.todays_goals_tree = GoalTree::empty();
            return;
        };

        dbg_log!("=== load_todays_goals ===");
        self.client.set_org_id(Some(org_id.clone()));

        match self.api_call(self.client.get_todays_goals(&org_id, None, None)) {
            Ok(resp) => {
                dbg_log!("Total nodes loaded: {}", resp.data.nodes.len());
                dbg_log!("Auto-generated count: {}", resp.data.auto_generated_count);

                for node in &resp.data.nodes {
                    dbg_log!(
                        "  Goal: {} | id: {} | depth: {} | is_direct: {} | owner: {}",
                        node.title,
                        node.id,
                        node.depth,
                        node.is_direct_assignment,
                        node.owner
                            .as_ref()
                            .map(|o| o.name.as_str())
                            .unwrap_or("None")
                    );
                }

                self.todays_goals_tree = GoalTree::from_todays_goal_nodes(resp.data.nodes);

                dbg_log!(
                    "Today's goals tree has {} roots",
                    self.todays_goals_tree.roots.len()
                );
            }
            Err(e) => {
                self.todays_goals_tree = GoalTree::empty();
                self.error_message = Some(format!("Failed to load today's goals: {e}"));
            }
        }
    }

    fn members_cursor_up(&mut self) {
        if self.members_cursor > 0 {
            self.members_cursor -= 1;
        }
    }

    fn members_cursor_down(&mut self) {
        if self.members_cursor + 1 < self.members_list.len() {
            self.members_cursor += 1;
        }
    }

    pub(super) fn adjust_members_scroll(&mut self, viewport_height: usize) {
        if viewport_height == 0 {
            return;
        }
        if self.members_cursor < self.members_scroll_offset {
            self.members_scroll_offset = self.members_cursor;
        } else if self.members_cursor >= self.members_scroll_offset + viewport_height {
            self.members_scroll_offset = self.members_cursor - viewport_height + 1;
        }
    }

    fn selected_goal_context(&self) -> Option<(String, String)> {
        let tree = self.active_goal_tree();
        let rows = tree.flatten();

        // 成果物行を選択していても、その成果物が属するゴール行まで遡って
        // 実際のタイトルを使う（IDではなくタイトルで提示する方針）。
        for row in rows.iter().take(tree.cursor + 1).rev() {
            if let TreeRow::Goal { goal_id, title, .. } = row {
                return Some((goal_id.to_string(), title.to_string()));
            }
        }
        None
    }

    fn selected_deliverable_context(
        &self,
    ) -> Option<(String, String, String, String, DeliverableType)> {
        let tree = self.active_goal_tree();
        let rows = tree.flatten();
        match rows.get(tree.cursor) {
            Some(TreeRow::DeliverableItem { deliverable, .. }) => Some((
                deliverable.objective_id.clone(),
                deliverable.id.clone(),
                deliverable.display_name.clone(),
                deliverable
                    .parent_deliverable_id
                    .clone()
                    .unwrap_or_default(),
                deliverable.node_type.clone(),
            )),
            _ => None,
        }
    }

    /// カーソル上のゴール行の (goal_id, title, is_completed)。
    fn selected_goal_row_context(&self) -> Option<(String, String, bool)> {
        let tree = self.active_goal_tree();
        let rows = tree.flatten();
        match rows.get(tree.cursor) {
            Some(TreeRow::Goal {
                goal_id,
                title,
                is_completed,
                ..
            }) => Some((goal_id.to_string(), title.to_string(), *is_completed)),
            _ => None,
        }
    }

    /// カーソル上のコメント行の情報。返り値は
    /// (goal_id, comment_id, 本文, 解決済みか)。
    /// goal_id はコメントの commentable_id（= ゴールID）から得る。
    fn selected_comment_context(&self) -> Option<(String, String, String, bool)> {
        let tree = self.active_goal_tree();
        let rows = tree.flatten();
        match rows.get(tree.cursor) {
            Some(TreeRow::CommentItem { comment, .. }) => Some((
                comment.commentable_id.clone(),
                comment.id.clone(),
                comment.content.clone(),
                comment.resolved_at.is_some(),
            )),
            _ => None,
        }
    }

    fn deliverable_folder_targets_for_goal(
        &self,
        goal_id: &str,
        current_deliverable_id: &str,
    ) -> Vec<(Option<String>, String)> {
        let tree = self.active_goal_tree();
        let rows = tree.flatten();
        let mut targets = vec![(None, "(root)".to_string())];

        for row in rows {
            if let TreeRow::DeliverableItem { deliverable, .. } = row
                && deliverable.objective_id == goal_id
                && deliverable.id != current_deliverable_id
                && deliverable.node_type == DeliverableType::Folder
            {
                targets.push((
                    Some(deliverable.id.clone()),
                    format!("{} ({})", deliverable.display_name, deliverable.id),
                ));
            }
        }

        targets
    }

    fn start_action_menu(&mut self) {
        // コメント行が最も具体的なので先に判定する。
        if let Some((_, _, content, resolved)) = self.selected_comment_context() {
            let mut items = vec![ActionMenuItem::ReplyComment];
            if resolved {
                items.push(ActionMenuItem::UnresolveComment);
            } else {
                items.push(ActionMenuItem::ResolveComment);
            }
            items.push(ActionMenuItem::ReactComment);
            items.push(ActionMenuItem::EditComment);
            items.push(ActionMenuItem::DeleteComment);
            self.modal_state = Some(ModalState::ActionMenu {
                title: format!("comment: {}", truncate_comment(&content)),
                items,
                selected_index: 0,
            });
            return;
        }

        if let Some((_, deliverable_id, deliverable_name, _, node_type)) =
            self.selected_deliverable_context()
        {
            let mut items = Vec::new();
            if node_type == DeliverableType::Document {
                items.push(ActionMenuItem::UpdateDeliverable);
            }
            items.push(ActionMenuItem::RenameDeliverable);
            items.push(ActionMenuItem::MoveDeliverable);
            items.push(ActionMenuItem::DeleteDeliverable);
            self.modal_state = Some(ModalState::ActionMenu {
                title: format!("{deliverable_name} ({deliverable_id})"),
                items,
                selected_index: 0,
            });
            return;
        }

        if let Some((_, goal_title, is_completed)) = self.selected_goal_row_context() {
            let complete_item = if is_completed {
                ActionMenuItem::ReopenGoal
            } else {
                ActionMenuItem::CompleteGoal
            };
            self.modal_state = Some(ModalState::ActionMenu {
                title: goal_title,
                items: vec![
                    ActionMenuItem::WorkWithCodex,
                    ActionMenuItem::WorkWithClaude,
                    complete_item,
                    ActionMenuItem::AddComment,
                    ActionMenuItem::AddDeliverable,
                    ActionMenuItem::EditGoal,
                    ActionMenuItem::DeleteGoal,
                ],
                selected_index: 0,
            });
            return;
        }

        self.error_message =
            Some("Select a goal, deliverable or comment to open actions".to_string());
    }

    /// 選択中ゴールの文脈を注入して、埋め込み codex セッションを起動する。
    fn start_codex(&mut self) {
        self.start_agent(AgentKind::Codex);
    }

    fn start_agent(&mut self, kind: AgentKind) {
        let name = kind.display_name();
        let label = kind.label();
        let Some((goal_id, title)) = self.selected_goal_context() else {
            self.error_message = Some(format!("ゴールを選択してから {label} を起動してください"));
            return;
        };

        // 未インストールでもクラッシュさせず、案内を出す。
        let bin = match kind {
            AgentKind::Codex => agent::codex_path(),
            AgentKind::ClaudeCode => agent::claude_path(),
        };
        let Some(agent_bin) = bin else {
            self.error_message = Some(match kind {
                AgentKind::Codex => {
                    "codex が見つかりません。`brew install codex` 等でインストールしてください"
                        .to_string()
                }
                AgentKind::ClaudeCode => {
                    "claude が見つかりません。`npm install -g @anthropic-ai/claude-code` 等でインストールしてください"
                        .to_string()
                }
            });
            return;
        };

        // DoD・ステータスを取得して左ペインの初期表示に使う。失敗しても空で続行する。
        let (dod, status_label, initial_body) = match self.api_call(self.client.get_goal(&goal_id))
        {
            Ok(resp) => {
                let goal = resp.data;
                let status =
                    GoalDisplayStatus::from_goal_state(goal.status.as_ref(), goal.is_completed)
                        .to_emoji_string();
                (goal.description.unwrap_or_default(), status, goal.body)
            }
            Err(_) => (String::new(), String::new(), None),
        };
        let initial_children = self
            .api_call(self.client.get_goal_children(&goal_id, 50, 0))
            .ok()
            .map(|resp| {
                resp.data
                    .children
                    .into_iter()
                    .map(child_goal_update_from_item)
                    .collect::<Vec<_>>()
            });

        // codex のサブプロセスから確実に呼べるよう、addness 自身の絶対パスを渡す。
        let addness_bin = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| "addness".to_string());
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        // 対象ゴールの軽量コンテキストは環境変数で伝える。起動直後は初期プロンプトを
        // 送らず、ユーザーがすぐ最初の指示を入力できる状態にする。
        match CodexPane::spawn(
            &agent_bin,
            &cwd,
            &addness_bin,
            goal_id,
            title,
            dod,
            status_label,
            kind,
        ) {
            Ok(mut pane) => {
                let body_loaded = pane.set_addness_body_context(initial_body);
                pane.push_activity(format!("{} {label} を起動", Local::now().format("%H:%M")));
                if body_loaded {
                    pane.last_addness_read_at = Some(Instant::now());
                    pane.last_addness_read_label = Some("body".to_string());
                    pane.push_activity(format!(
                        "{} 現状(body)を読込",
                        Local::now().format("%H:%M")
                    ));
                }
                if let Some(children) = initial_children {
                    let count = children.len();
                    pane.child_count = Some(count);
                    pane.update_children(children);
                    pane.last_addness_read_at = Some(Instant::now());
                    pane.last_addness_read_label = Some(if body_loaded {
                        "body/子ゴール".to_string()
                    } else {
                        "子ゴール".to_string()
                    });
                    pane.push_activity(format!(
                        "{} 子ゴール{count}件を読込（完了済み含む）",
                        Local::now().format("%H:%M")
                    ));
                }
                if pane.loaded_history_count() > 0 {
                    pane.push_activity(format!(
                        "{} 前回履歴を{}件復元",
                        Local::now().format("%H:%M"),
                        pane.loaded_history_count()
                    ));
                }
                if let Some(path) = pane.history_path_label() {
                    pane.push_activity(format!(
                        "{} 履歴保存: {path}",
                        Local::now().format("%H:%M")
                    ));
                }
                pane.push_activity(format!(
                    "{} 軽量コンテキストで入力待ち",
                    Local::now().format("%H:%M")
                ));
                pane.push_activity(format!(
                    "{} body/DoD/子ゴール/通知をここに表示",
                    Local::now().format("%H:%M")
                ));
                self.codex = Some(pane);
                self.active_pane = ActivePane::Codex;
                self.codex_terminal_area = None;
                self.codex_status_area = None;
                self.codex_contract_area = None;
                self.codex_activity_area = None;
                self.codex_contract_scroll = 0;
                self.codex_activity_scroll = 0;
                self.codex_last_scroll_input = None;
                // 通常UI → codex の構造遷移。前画面の残像を消すため全クリアする。
                self.needs_full_clear = true;
                // codex 画面上のトラックパッド/ホイール操作を受け取るため有効化（codex 中のみ）。
                Self::set_mouse_capture(true);
            }
            Err(e) => {
                self.error_message = Some(format!("{name} の起動に失敗しました: {e}"));
            }
        }
    }

    /// マウスキャプチャの ON/OFF。通常画面ではテキスト選択を壊さないよう OFF にする。
    fn set_mouse_capture(enable: bool) {
        let mut out = std::io::stdout();
        if enable {
            let _ = out.write_all(XTERM_MOUSE_CAPTURE_ON);
        } else {
            let _ = out.write_all(XTERM_MOUSE_CAPTURE_OFF);
        }
        let _ = out.flush();
    }

    /// codex セッションの作業状況を対象ゴールの body に自動記録する。
    /// body 全体は壊さず、専用ブロックだけを追記・差し替えする。
    fn record_codex_session_to_goal_body(
        &mut self,
        goal_id: &str,
        cwd: &str,
        session_state: &str,
        last_prompt: Option<&str>,
        summary: Option<&CodexWorkSummary>,
    ) -> CodexBodyRecordResult {
        let record = codex_work_memo(cwd, session_state, last_prompt, summary);

        let goal = match self.api_call(self.client.get_goal(goal_id)) {
            Ok(resp) => resp.data,
            Err(e) => {
                let message = format!("{e}");
                if is_permission_denied_error_text(&message) {
                    self.success_message =
                        Some("権限がないためCodex作業メモの自動記録をスキップしました".to_string());
                    return CodexBodyRecordResult::SkippedPermission;
                }
                self.error_message = Some(format!("Codex自動記録の取得に失敗しました: {message}"));
                return CodexBodyRecordResult::Failed;
            }
        };
        let req = codex_body_update_request(goal.body.as_deref(), &record);

        match self.api_call(self.client.update_goal(goal_id, &req)) {
            Ok(_) => {
                self.success_message =
                    Some("Codex作業メモをAddnessの現状(body)に書き込みました".to_string());
                CodexBodyRecordResult::Written
            }
            Err(e) => {
                let message = format!("{e}");
                if is_permission_denied_error_text(&message) {
                    self.success_message = Some(
                        "書き込み権限がないためCodex作業メモの自動記録をスキップしました"
                            .to_string(),
                    );
                    CodexBodyRecordResult::SkippedPermission
                } else {
                    self.error_message = Some(format!(
                        "Codex作業メモの現状(body)書き込みに失敗しました: {message}"
                    ));
                    CodexBodyRecordResult::Failed
                }
            }
        }
    }

    fn maybe_start_codex_prompt_body_record(&mut self) -> bool {
        if self.codex_body_record_job.is_some() {
            return false;
        }
        let Some((goal_id, cwd, prompt)) = self.codex.as_mut().and_then(|pane| {
            if pane.finished {
                return None;
            }
            let prompt = pane.prompt_needs_body_record()?.to_string();
            pane.mark_body_recorded_prompt();
            Some((pane.goal_id.clone(), pane.cwd.clone(), prompt))
        }) else {
            return false;
        };

        let client = self.client.clone();
        self.codex_body_record_job = Some(self.rt.spawn(async move {
            // git status/diff のサブプロセスは UI スレッドを固めないよう
            // ブロッキングプールで作る。
            let record = tokio::task::spawn_blocking(move || {
                codex_work_memo(&cwd, "依頼受付", Some(&prompt), None)
            })
            .await
            .unwrap_or_default();
            let result = async {
                let goal = client.get_goal(&goal_id).await?.data;
                let req = codex_body_update_request(goal.body.as_deref(), &record);
                client.update_goal(&goal_id, &req).await?;
                Ok::<(), anyhow::Error>(())
            }
            .await;

            match result {
                Ok(()) => CodexBodyRecordOutcome {
                    ok: true,
                    message: "現状(body)に作業メモを書込".to_string(),
                },
                Err(e) => {
                    let message = format!("{e}");
                    let message = if is_permission_denied_error_text(&message) {
                        "書き込み権限なしのため現状(body)の作業メモをスキップ".to_string()
                    } else {
                        format!("現状(body)の作業メモに失敗: {message}")
                    };
                    CodexBodyRecordOutcome { ok: false, message }
                }
            }
        }));
        if let Some(pane) = self.codex.as_mut() {
            let now = Local::now().format("%H:%M");
            pane.push_activity(format!("{now} 現状(body)の作業メモを予約"));
        }
        true
    }

    fn poll_codex_body_record_job(&mut self) -> bool {
        let Some(handle) = self.codex_body_record_job.as_ref() else {
            return false;
        };
        if !handle.is_finished() {
            return false;
        }
        let handle = self.codex_body_record_job.take().unwrap();
        let outcome = self
            .rt
            .block_on(handle)
            .unwrap_or_else(|e| CodexBodyRecordOutcome {
                ok: false,
                message: format!("Codex作業メモに失敗: {e}"),
            });
        if let Some(pane) = self.codex.as_mut() {
            let now = Local::now().format("%H:%M");
            if outcome.ok {
                pane.last_addness_write_at = Some(Instant::now());
            }
            pane.push_activity(format!("{now} {}", outcome.message));
        }
        true
    }

    fn maybe_start_codex_turn_body_record(&mut self) -> bool {
        if self.codex_body_record_job.is_some() {
            return false;
        }
        let Some((goal_id, cwd, prompt, summary, turn)) = self.codex.as_mut().and_then(|pane| {
            let record = pane.take_completed_turn_body_record()?;
            Some((
                pane.goal_id.clone(),
                pane.cwd.clone(),
                record.prompt,
                record.summary,
                record.turn,
            ))
        }) else {
            return false;
        };

        let client = self.client.clone();
        self.codex_body_record_job = Some(self.rt.spawn(async move {
            let session_state = format!("turn {turn}完了");
            let record = tokio::task::spawn_blocking(move || {
                codex_work_memo(&cwd, &session_state, prompt.as_deref(), Some(&summary))
            })
            .await
            .unwrap_or_default();
            let result = async {
                let goal = client.get_goal(&goal_id).await?.data;
                let req = codex_body_update_request(goal.body.as_deref(), &record);
                client.update_goal(&goal_id, &req).await?;
                Ok::<(), anyhow::Error>(())
            }
            .await;

            match result {
                Ok(()) => CodexBodyRecordOutcome {
                    ok: true,
                    message: format!("現状(body)にturn {turn}完了メモを書込"),
                },
                Err(e) => {
                    let message = format!("{e}");
                    let message = if is_permission_denied_error_text(&message) {
                        format!("書き込み権限なしのためturn {turn}完了メモをスキップ")
                    } else {
                        format!("turn {turn}完了メモに失敗: {message}")
                    };
                    CodexBodyRecordOutcome { ok: false, message }
                }
            }
        }));
        if let Some(pane) = self.codex.as_mut() {
            let now = Local::now().format("%H:%M");
            pane.push_activity(format!("{now} turn {turn}完了メモを予約"));
        }
        true
    }

    fn maybe_record_finished_codex_session(&mut self) -> bool {
        // 非同期の作業メモ記録が進行中の間は、同じ body を二重に read-modify-write して
        // 互いの記録を上書きし合わないよう、ジョブ完了（poll で take）を待ってから終了記録する。
        if self.codex_body_record_job.is_some() {
            return false;
        }
        let Some((goal_id, cwd, last_prompt, summary)) = self.codex.as_mut().and_then(|pane| {
            if pane.finished && !pane.auto_record_attempted {
                pane.auto_record_attempted = true;
                let (last_prompt, summary) = pane.final_body_record_context();
                Some((pane.goal_id.clone(), pane.cwd.clone(), last_prompt, summary))
            } else {
                None
            }
        }) else {
            return false;
        };

        let result = self.record_codex_session_to_goal_body(
            &goal_id,
            &cwd,
            "codex終了",
            last_prompt.as_deref(),
            Some(&summary),
        );
        if let Some(pane) = self.codex.as_mut() {
            let now = Local::now().format("%H:%M");
            match result {
                CodexBodyRecordResult::Written => {
                    pane.last_addness_write_at = Some(Instant::now());
                    pane.push_activity(format!("{now} 現状(body)に終了メモを書込"));
                }
                CodexBodyRecordResult::SkippedPermission => {
                    pane.push_activity(format!("{now} 書き込み権限なしのため終了メモをスキップ"));
                }
                CodexBodyRecordResult::Failed => {
                    pane.push_activity(format!("{now} 現状(body)の終了メモに失敗"));
                }
            }
        }
        true
    }

    /// codex ペインを閉じる（プロセスを終了させて通常画面へ戻る）。
    /// codex が Addness に書き戻した子ゴール等を反映するため、ツリーを再読込する。
    fn close_codex(&mut self) {
        // 通常画面に戻るのでマウスキャプチャを解除（テキスト選択を戻す）。
        Self::set_mouse_capture(false);
        self.codex_terminal_area = None;
        self.codex_status_area = None;
        self.codex_contract_area = None;
        self.codex_activity_area = None;
        self.codex_contract_scroll = 0;
        self.codex_activity_scroll = 0;
        self.codex_last_scroll_input = None;
        // 進行中の非同期作業メモ記録を先に止め、この後の同期終了記録と body を奪い合わせない。
        if let Some(job) = self.codex_body_record_job.take() {
            job.abort();
        }
        if let Some(mut pane) = self.codex.take() {
            if !pane.auto_record_attempted {
                pane.auto_record_attempted = true;
                let (last_prompt, summary) = pane.final_body_record_context();
                let state = if pane.finished {
                    "codex終了"
                } else {
                    "codex中断/ペイン終了"
                };
                self.record_codex_session_to_goal_body(
                    &pane.goal_id,
                    &pane.cwd,
                    state,
                    last_prompt.as_deref(),
                    Some(&summary),
                );
            }
            pane.kill();
        }
        if let Some(mut job) = self.codex_dod_job.take() {
            job.cleanup();
        }
        self.codex_refresh = None;
        self.last_codex_refresh = None;
        self.active_pane = ActivePane::Content;
        // codex → 通常UI の構造遷移。前画面の残像を消すため全クリアする。
        self.needs_full_clear = true;
        // ツリー再読込はブロッキングなので即時には行わず、UI を先に切り替えてから
        // 次フレームで実行する（F12 の体感を速くする）。
        self.pending_codex_tree_reload = true;
    }

    /// JSONL 出力の取り込みとプロセス終了検知。codex 起動中に毎フレーム呼ぶ。
    /// あわせて契約ペイン（DoD/タイトル）のライブ更新と DoD 判定を駆動する。
    /// 画面に影響する変化があれば `true` を返す（再描画判定に使う）。
    fn update_codex(&mut self) -> bool {
        let mut changed = false;
        let mut close_after_exit_command = false;
        let mut terminal_notice = None;
        if let Some(pane) = self.codex.as_mut() {
            changed |= pane.update();
            close_after_exit_command = pane.should_close_after_exit_command();
            terminal_notice = pane.take_terminal_notice();
        }
        if let Some(notice) = terminal_notice {
            emit_terminal_notification(&notice);
            changed = true;
        }
        changed |= self.maybe_start_codex_prompt_body_record();
        changed |= self.poll_codex_body_record_job();
        changed |= self.maybe_start_codex_turn_body_record();
        changed |= self.maybe_record_finished_codex_session();
        if close_after_exit_command {
            self.close_codex();
            return true;
        }
        changed |= self.poll_codex_refresh();
        self.maybe_start_codex_refresh();
        changed |= self.poll_dod_job();
        changed
    }

    /// `codex exec` を read-only サンドボックスで起動し、各 DoD 項目が現在の
    /// 作業ツリーで満たされているかを JSON Schema 付きで判定させる（還流: DoDチェック）。
    fn start_dod_assessment(&mut self) {
        if self.codex_dod_job.is_some() {
            return;
        }
        let Some(pane) = self.codex.as_ref() else {
            return;
        };
        if pane.dod_items.is_empty() {
            self.error_message = Some("DoD が未設定のため判定できません".to_string());
            return;
        }
        let Some(codex_bin) = agent::codex_path() else {
            self.error_message = Some("codex が見つかりません".to_string());
            return;
        };

        let items = pane.dod_items.clone();
        let goal_id = pane.goal_id.clone();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let tmp = std::env::temp_dir();
        // プロセスIDを混ぜて、複数の TUI セッションが同一ゴールを判定しても衝突しないようにする。
        let pid = std::process::id();
        let schema_path = tmp.join(format!("addness-dod-schema-{goal_id}-{pid}.json"));
        let out_path = tmp.join(format!("addness-dod-out-{goal_id}-{pid}.json"));

        if std::fs::write(&schema_path, agent::dod_assessment_schema()).is_err() {
            self.error_message = Some("一時ファイルの書き込みに失敗しました".to_string());
            return;
        }
        let _ = std::fs::remove_file(&out_path);

        let prompt = agent::build_dod_assessment_prompt(&items);
        let child = std::process::Command::new(&codex_bin)
            .arg("exec")
            .args(["-s", "read-only", "--color", "never"])
            .arg("--output-schema")
            .arg(&schema_path)
            .arg("-o")
            .arg(&out_path)
            .arg("-C")
            .arg(&cwd)
            .arg(&prompt)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        match child {
            Ok(child) => {
                self.codex_dod_job = Some(DodJob {
                    child,
                    out_path,
                    schema_path,
                    item_count: items.len(),
                    started: Instant::now(),
                });
                if let Some(pane) = self.codex.as_mut() {
                    pane.assessing = true;
                }
                self.success_message = Some("DoD 判定を実行中…".to_string());
            }
            Err(e) => {
                // 起動失敗時は書き込んだスキーマファイルを残さない。
                let _ = std::fs::remove_file(&schema_path);
                self.error_message = Some(format!("DoD 判定の起動に失敗しました: {e}"));
            }
        }
    }

    /// DoD 判定ジョブの完了を非ブロッキングで確認し、結果を契約ペインへ反映する。
    /// タイムアウト超過時は強制終了する。完了/失敗いずれでも一時ファイルを掃除する。
    /// 状態が変化した場合は `true` を返す（再描画判定に使う）。
    fn poll_dod_job(&mut self) -> bool {
        let Some(job) = self.codex_dod_job.as_mut() else {
            return false;
        };

        let status = match job.child.try_wait() {
            Ok(None) => {
                // まだ実行中。タイムアウト超過なら打ち切る。
                if job.started.elapsed() >= DOD_ASSESSMENT_TIMEOUT {
                    let mut job = self.codex_dod_job.take().unwrap();
                    job.cleanup();
                    self.set_codex_assessing(false);
                    self.error_message = Some("DoD 判定がタイムアウトしました".to_string());
                    return true;
                }
                return false;
            }
            Ok(Some(status)) => status,
            Err(_) => {
                let mut job = self.codex_dod_job.take().unwrap();
                job.cleanup();
                self.set_codex_assessing(false);
                return true;
            }
        };

        // 完了。結果を読み出してから一時ファイルを掃除する。
        let mut job = self.codex_dod_job.take().unwrap();
        let result = if status.success() {
            std::fs::read_to_string(&job.out_path)
                .ok()
                .and_then(|content| parse_dod_results(&content, job.item_count))
        } else {
            None
        };
        job.cleanup();
        self.set_codex_assessing(false);

        match result {
            Some(results) if !results.is_empty() => {
                if let Some(pane) = self.codex.as_mut() {
                    pane.apply_dod_results(&results);
                    // 達成数・総数は実際の項目数とチェック状態から数える
                    // （codex が一部省略・重複しても表示が破綻しないように）。
                    let met = pane.dod_checks.iter().filter(|c| **c == Some(true)).count();
                    let total = pane.dod_items.len();
                    self.success_message = Some(format!("DoD 判定完了: {met}/{total} 達成"));
                }
            }
            Some(_) => {
                // 空の results。judge が判定できなかったとみなす。
                self.error_message = Some("DoD 判定結果が空でした".to_string());
            }
            None if status.success() => {
                self.error_message = Some("DoD 判定結果の解析に失敗しました".to_string());
            }
            None => {
                self.error_message = Some("DoD 判定に失敗しました".to_string());
            }
        }
        true
    }

    /// 契約ペインの「判定中」フラグを設定する小ヘルパー。
    fn set_codex_assessing(&mut self, value: bool) {
        if let Some(pane) = self.codex.as_mut() {
            pane.assessing = value;
        }
    }

    /// 一定間隔（3秒）ごとに、対象ゴールの再取得をバックグラウンドで開始する。
    /// codex が CLI 経由で書き戻した DoD 更新を契約ペインに反映するため。
    fn maybe_start_codex_refresh(&mut self) {
        let Some(pane) = self.codex.as_ref() else {
            return;
        };
        // 終了後は還流フェーズなので、ここでのポーリングは止める。
        if pane.finished || self.codex_refresh.is_some() {
            return;
        }
        let due = self
            .last_codex_refresh
            .is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(3));
        if !due {
            return;
        }
        let goal_id = pane.goal_id.clone();
        let client = self.client.clone();
        self.codex_refresh = Some(self.rt.spawn(async move {
            // ゴール本体・子ゴール数・コメント数を 1 往復に畳んで取得する。
            // 子ゴール／コメントの取得は失敗しても本体があれば続行する。
            let (goal_res, children_res, comments_res, deliverables_res) = tokio::join!(
                client.get_goal(&goal_id),
                client.get_goal_children(&goal_id, 50, 0),
                client.list_comments(&goal_id),
                client.get_goal_deliverables(&goal_id),
            );
            let goal = goal_res.ok()?.data;
            let status_label =
                GoalDisplayStatus::from_goal_state(goal.status.as_ref(), goal.is_completed)
                    .to_emoji_string();
            let children = children_res.ok().map(|r| {
                r.data
                    .children
                    .into_iter()
                    .map(child_goal_update_from_item)
                    .collect::<Vec<_>>()
            });
            let comment_count = comments_res.ok().map(|r| r.total_count.max(0) as usize);
            let deliverables = deliverables_res.ok().map(|r| r.data.deliverables);
            let deliverable_count = deliverables.as_ref().map(Vec::len);
            let trace_links = deliverables
                .unwrap_or_default()
                .into_iter()
                .filter_map(|d| codex_trace_link_label(&d.display_name, d.link_url.as_deref()))
                .take(3)
                .collect::<Vec<_>>();
            Some(CodexSnapshot {
                title: goal.title,
                description: goal.description.unwrap_or_default(),
                body: goal.body,
                status_label,
                comment_count,
                deliverable_count,
                trace_links,
                children,
            })
        }));
        self.last_codex_refresh = Some(Instant::now());
    }

    /// 進行中の再取得タスクが完了していれば、契約ペインへ反映する（非ブロッキング）。
    /// 反映して画面が変わった場合は `true` を返す。
    fn poll_codex_refresh(&mut self) -> bool {
        let Some(handle) = self.codex_refresh.as_ref() else {
            return false;
        };
        if !handle.is_finished() {
            return false;
        }
        let handle = self.codex_refresh.take().unwrap();
        // is_finished が真なので block_on は即座に返る。
        let synced = if let Ok(Some(snap)) = self.rt.block_on(handle) {
            if let Some(pane) = self.codex.as_mut() {
                let now = Local::now().format("%H:%M");

                // ステータス変化を検知して更新ログに残す（Addness 側の進行を可視化）。
                if snap.status_label != pane.status_label {
                    pane.status_label = snap.status_label.clone();
                    pane.status_changed_at = Some(Instant::now());
                    pane.push_activity(format!("{now} ステータス → {}", snap.status_label));
                }

                if snap.title != pane.goal_title {
                    pane.goal_title = snap.title;
                    pane.push_activity(format!("{now} タイトルを書込反映"));
                }

                // DoD 判定の実行中は項目とチェックを作り直さない（番号ずれ防止）。
                if !pane.assessing && pane.set_dod(snap.description) {
                    pane.dod_changed_at = Some(Instant::now());
                    pane.push_activity(format!("{now} 方針(DoD)を書込反映"));
                }

                if pane.set_addness_body_context(snap.body) {
                    pane.last_addness_read_at = Some(Instant::now());
                    pane.last_addness_read_label = Some("body".to_string());
                    pane.push_activity(format!("{now} 現状(body)を同期"));
                }

                // 子ゴール一覧の差し替え＋増加検知（codex が Addness に書き込んだサイン）。
                if let Some(children) = snap.children {
                    let new_n = children.len();
                    let old_n = pane.child_count.unwrap_or(0);
                    if pane.child_count.is_some() && new_n > old_n {
                        pane.push_activity(format!(
                            "{now} 子ゴール +{} (計{new_n})",
                            new_n - old_n
                        ));
                    }
                    pane.child_count = Some(new_n);
                    pane.update_children(children);
                }
                if let Some(new_n) = snap.comment_count {
                    match pane.comment_count {
                        Some(old_n) if new_n > old_n => {
                            pane.push_activity(format!(
                                "{now} コメント/通知 +{} (計{new_n})",
                                new_n - old_n
                            ));
                        }
                        _ => {}
                    }
                    pane.comment_count = Some(new_n);
                }
                if let Some(new_n) = snap.deliverable_count {
                    match pane.deliverable_count {
                        Some(old_n) if new_n > old_n => {
                            pane.push_activity(format!(
                                "{now} 成果物 +{} (計{new_n})",
                                new_n - old_n
                            ));
                        }
                        _ => {}
                    }
                    pane.deliverable_count = Some(new_n);
                }
                if snap.trace_links != pane.trace_links {
                    pane.trace_links = snap.trace_links;
                    if !pane.trace_links.is_empty() {
                        pane.push_activity(format!("{now} PR/Release traceを更新"));
                    }
                }
                true
            } else {
                false
            }
        } else {
            false
        };
        if synced {
            // 同期完了自体を変化として扱い、鼓動表示（最終同期時刻＋スピナー）を更新させる。
            self.last_codex_sync = Some(Instant::now());
            self.codex_sync_tick = self.codex_sync_tick.wrapping_add(1);
        }
        synced
    }

    /// codex ログのスクロールキーを処理する。
    /// `codex exec --json` の右ペインは Addness 側のリストなので、実行中でも
    /// 通常の矢印・PgUp/PgDn・Home/End で履歴を遡れる。
    fn handle_codex_log_scroll(
        pane: &mut CodexPane,
        key: KeyEvent,
        allow_plain: bool,
        allow_vim_keys: bool,
    ) -> bool {
        let plain = key.modifiers.is_empty();
        if !allow_plain || !plain {
            return false;
        }
        let page = pane.page() as isize;

        match key.code {
            KeyCode::Up => {
                pane.scroll_lines(1);
                return true;
            }
            KeyCode::Down => {
                pane.scroll_lines(-1);
                return true;
            }
            KeyCode::PageUp => {
                pane.scroll_lines(page);
                return true;
            }
            KeyCode::PageDown => {
                pane.scroll_lines(-page);
                return true;
            }
            KeyCode::Home => {
                pane.scroll_to_top();
                return true;
            }
            KeyCode::End => {
                pane.scroll_to_live();
                return true;
            }
            KeyCode::Char('k') if allow_vim_keys => {
                pane.scroll_lines(1);
                return true;
            }
            KeyCode::Char('j') if allow_vim_keys => {
                pane.scroll_lines(-1);
                return true;
            }
            KeyCode::Char('u') if allow_vim_keys => {
                pane.scroll_lines(page);
                return true;
            }
            KeyCode::Char('d') if allow_vim_keys => {
                pane.scroll_lines(-page);
                return true;
            }
            KeyCode::Char('g') if allow_vim_keys => {
                pane.scroll_to_top();
                return true;
            }
            KeyCode::Char('G') if allow_vim_keys => {
                pane.scroll_to_live();
                return true;
            }
            _ => {}
        }

        false
    }

    fn is_codex_shift_navigation_key(key: KeyEvent) -> bool {
        key.modifiers == KeyModifiers::SHIFT
            && matches!(
                key.code,
                KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::PageUp
                    | KeyCode::PageDown
                    | KeyCode::Home
                    | KeyCode::End
            )
    }

    /// codex ペインへのキー入力処理。
    /// 実行中は F12 で終了、trackpad/wheel でログをスクロールし、それ以外のキーは codex へ転送する。
    /// 終了後は還流バー（c/s/d）で成果を Addness に書き戻し、Esc/q で閉じる。
    fn handle_codex_key(&mut self, key: KeyEvent) {
        // codex フォーカス中でも、入力欄が空で各種パレット・検索・ピッカー非表示なら
        // `?` でヘルプオーバーレイを開けるようにする。入力途中の `?` は文字入力のまま。
        if self.codex_help_key_eligible(key) {
            self.show_help = true;
            self.help_scroll = 0;
            return;
        }
        if let Some(pane) = self.codex.as_mut() {
            if pane.handle_search_key(key) {
                return;
            }
            if pane.handle_decision_key(key) {
                return;
            }
            if pane.turn_picker_open() {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        pane.close_turn_picker();
                        return;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        pane.move_turn_picker_selection(-1);
                        return;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        pane.move_turn_picker_selection(1);
                        return;
                    }
                    KeyCode::Enter | KeyCode::Char('o') => {
                        pane.open_selected_turn_from_picker();
                        return;
                    }
                    KeyCode::Char('c') => {
                        pane.close_selected_turn_from_picker();
                        return;
                    }
                    KeyCode::Char(' ') => {
                        pane.toggle_selected_turn_from_picker();
                        return;
                    }
                    KeyCode::Char('a') => {
                        pane.open_all_turns();
                        return;
                    }
                    _ => return,
                }
            }
            // スラッシュコマンドパレット表示中は ↑↓ で候補選択・Tab で補完する。
            // Esc / 文字入力はそのまま入力欄へ流し、既存挙動（入力クリア等）に委ねる。
            if pane.slash_palette_active() && key.modifiers.is_empty() {
                match key.code {
                    KeyCode::Up => {
                        pane.move_slash_palette_selection(-1);
                        return;
                    }
                    KeyCode::Down => {
                        pane.move_slash_palette_selection(1);
                        return;
                    }
                    KeyCode::Tab => {
                        pane.accept_slash_palette_selection();
                        return;
                    }
                    _ => {}
                }
            }
            // @メンションのファイル候補パレット表示中は ↑↓ で選択、Tab/Enter で確定、Esc で閉じる。
            // ディレクトリ確定時は潜って絞り込みを続け、ファイル確定でパスを挿入する。
            if pane.mention_palette_active() && key.modifiers.is_empty() {
                match key.code {
                    KeyCode::Up => {
                        pane.move_mention_palette_selection(-1);
                        return;
                    }
                    KeyCode::Down => {
                        pane.move_mention_palette_selection(1);
                        return;
                    }
                    KeyCode::Tab | KeyCode::Enter => {
                        pane.accept_mention_palette_selection();
                        return;
                    }
                    KeyCode::Esc => {
                        pane.dismiss_mention_palette();
                        return;
                    }
                    _ => {}
                }
            }
            if key.modifiers.is_empty() {
                match key.code {
                    KeyCode::F(2) => {
                        pane.cycle_model();
                        return;
                    }
                    KeyCode::F(3) => {
                        pane.cycle_reasoning();
                        return;
                    }
                    KeyCode::F(4) => {
                        pane.cycle_approval();
                        return;
                    }
                    KeyCode::F(5) => {
                        pane.cycle_sandbox();
                        return;
                    }
                    KeyCode::F(6) => {
                        pane.toggle_diff_view();
                        return;
                    }
                    KeyCode::F(7) => {
                        pane.open_turn_picker();
                        return;
                    }
                    _ => {}
                }
            }
            if key.modifiers == KeyModifiers::ALT && matches!(key.code, KeyCode::Char('e' | 'E')) {
                pane.toggle_visible_turn_collapsed();
                return;
            }
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match key.code {
                    KeyCode::Char('t' | 'T') => {
                        pane.cycle_log_filter();
                        return;
                    }
                    KeyCode::Char('f' | 'F') => {
                        pane.begin_search();
                        return;
                    }
                    KeyCode::Char('l' | 'L') => {
                        pane.clear_search();
                        return;
                    }
                    KeyCode::Char('o' | 'O') | KeyCode::Enter | KeyCode::Char(' ') => {
                        pane.toggle_visible_turn_collapsed();
                        return;
                    }
                    KeyCode::Char('e' | 'E') => {
                        pane.toggle_old_turns_collapsed();
                        return;
                    }
                    _ => {}
                }
            }
            if pane.decision_banner().is_none()
                && pane.scrollback == 0
                && !pane.finished
                && pane.input_line().is_empty()
                && key.modifiers.is_empty()
                && matches!(key.code, KeyCode::Enter | KeyCode::Char(' '))
            {
                pane.toggle_visible_turn_collapsed();
                return;
            }
            if pane.decision_banner().is_none()
                && (pane.scrollback > 0 || pane.finished)
                && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT)
            {
                match key.code {
                    KeyCode::Enter | KeyCode::Char(' ') if key.modifiers.is_empty() => {
                        pane.toggle_visible_turn_collapsed();
                        return;
                    }
                    KeyCode::Char('e') => {
                        pane.toggle_visible_turn_collapsed();
                        return;
                    }
                    KeyCode::Char('E') => {
                        pane.toggle_old_turns_collapsed();
                        return;
                    }
                    _ => {}
                }
            }
        }

        let finished = self.codex.as_ref().map(|c| c.finished).unwrap_or(true);
        if finished {
            // 還流アクションのキー操作時は、古いステータスメッセージを消して鮮度を保つ
            // （codex フォーカス中は通常のキー処理を通らずクリアされないため）。
            if let KeyCode::Char('c' | 's' | 'd' | 'v') = key.code {
                self.error_message = None;
                self.success_message = None;
            }
            if key.modifiers.is_empty() {
                match key.code {
                    KeyCode::Char('c') => {
                        self.start_codex_reflow_comment();
                        return;
                    }
                    KeyCode::Char('s') => {
                        self.start_codex_reflow_edit();
                        return;
                    }
                    KeyCode::Char('d') => {
                        self.start_codex_reflow_deliverable();
                        return;
                    }
                    KeyCode::Char('v') => {
                        self.start_dod_assessment();
                        return;
                    }
                    KeyCode::Esc | KeyCode::Char('q') => {
                        self.close_codex();
                        return;
                    }
                    _ => {}
                }
            }
            // 終了後は codex がキーを処理しないので、ログを遡れるようにする。
            if let Some(pane) = self.codex.as_mut()
                && Self::handle_codex_log_scroll(pane, key, true, true)
            {
                return;
            }
            return;
        }
        if key.code == KeyCode::F(12) {
            self.close_codex();
            return;
        }
        if key.code == KeyCode::F(9) {
            self.send_codex_resume_prompt();
            return;
        }
        if let Some(pane) = self.codex.as_mut() {
            if Self::is_codex_shift_navigation_key(key) {
                return;
            }
            if pane.scrollback > 0 && key.code == KeyCode::Esc {
                pane.scroll_to_live();
                return;
            }
            // ターン実行中は Esc 2回でターンを中断する（誤爆防止に 1回目は警告表示）。
            // 入力欄が空・スクロール中でない・各種パレット非表示のときだけ拾い、
            // それ以外（入力途中の Esc など）は既存の入力クリア挙動へ委ねる。
            if pane.is_turn_running()
                && pane.decision_banner().is_none()
                && !pane.slash_palette_active()
                && !pane.mention_palette_active()
                && pane.scrollback == 0
                && pane.input_line().is_empty()
                && key.code == KeyCode::Esc
                && key.modifiers.is_empty()
            {
                if pane.take_esc_interrupt_armed() {
                    pane.interrupt_turn_by_esc();
                } else {
                    pane.arm_esc_interrupt();
                }
                return;
            }
            if pane.scrollback == 0 && pane.captures_input_key(key) {
                pane.input(key);
                return;
            }
            if Self::handle_codex_log_scroll(pane, key, true, false) {
                return;
            }
            // 過去ログを見たまま通常入力すると入力位置が見えないので、入力前にライブへ戻す。
            if pane.scrollback > 0 {
                pane.scroll_to_live();
            }
            pane.input(key);
        }
    }

    /// codex フォーカス中に `?` でヘルプを開いてよいか。パレット・検索・ターンピッカー・
    /// 判定バナー表示中やスクロール中、入力欄に文字がある場合は対象外（従来どおり文字入力）。
    fn codex_help_key_eligible(&self, key: KeyEvent) -> bool {
        if key.code != KeyCode::Char('?') || !key.modifiers.is_empty() {
            return false;
        }
        let Some(pane) = self.codex.as_ref() else {
            return false;
        };
        !pane.is_search_editing()
            && !pane.slash_palette_active()
            && !pane.mention_palette_active()
            && !pane.turn_picker_open()
            && pane.decision_banner().is_none()
            && pane.scrollback == 0
            && pane.input_line().is_empty()
    }

    fn handle_codex_paste(&mut self, text: &str) {
        if let Some(pane) = self.codex.as_mut() {
            if pane.scrollback > 0 {
                pane.scroll_to_live();
            }
            pane.paste_input(text);
        }
    }

    fn handle_modal_paste(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        for ch in normalized.chars() {
            let ch = if matches!(ch, '\n' | '\t') { ' ' } else { ch };
            self.handle_modal_input(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
    }

    fn send_codex_resume_prompt(&mut self) {
        if let Some(pane) = self.codex.as_mut() {
            if pane.scrollback > 0 {
                pane.scroll_to_live();
            }
            pane.submit_system_line(agent::resume_prompt());
            let now = Local::now().format("%H:%M");
            pane.push_activity(format!("{now} F9 再開プロンプトを送信"));
            self.success_message = Some("Addnessから再開するプロンプトを送信しました".to_string());
        }
    }

    fn handle_codex_mouse_batch(&mut self, first: MouseEvent) -> Result<()> {
        let mut latest_scroll = None;
        let mut delta = 0;
        let mut event_count = 0;
        Self::collect_mouse_scroll(first, &mut latest_scroll, &mut delta, &mut event_count);

        let mut pending_event = None;
        for _ in 0..CODEX_MOUSE_DRAIN_LIMIT {
            if !event::poll(Duration::from_millis(0))? {
                break;
            }

            match event::read()? {
                Event::Mouse(mouse) => {
                    Self::collect_mouse_scroll(
                        mouse,
                        &mut latest_scroll,
                        &mut delta,
                        &mut event_count,
                    );
                }
                event => {
                    pending_event = Some(event);
                    break;
                }
            }
        }

        if let Some(mut mouse) = latest_scroll {
            if delta == 0 {
                self.codex_last_scroll_input = Some(format!(
                    "mouse {:?} x{event_count} -> coalesced",
                    mouse.kind
                ));
            } else {
                mouse.kind = Self::mouse_kind_for_delta(delta);
                self.handle_codex_mouse_scroll(mouse, delta, event_count);
            }
        }

        if let Some(event) = pending_event {
            self.handle_event(event)?;
        }

        Ok(())
    }

    fn collect_mouse_scroll(
        mouse: MouseEvent,
        latest_scroll: &mut Option<MouseEvent>,
        delta: &mut isize,
        event_count: &mut usize,
    ) {
        if let Some(next_delta) = Self::mouse_scroll_delta(mouse.kind) {
            *latest_scroll = Some(mouse);
            *delta += next_delta;
            *event_count += 1;
        }
    }

    fn mouse_kind_for_delta(delta: isize) -> MouseEventKind {
        if delta >= 0 {
            MouseEventKind::ScrollUp
        } else {
            MouseEventKind::ScrollDown
        }
    }

    fn handle_codex_mouse_scroll(&mut self, mouse: MouseEvent, delta: isize, event_count: usize) {
        if Self::point_in_area(self.codex_terminal_area, mouse.column, mouse.row) {
            let Some(area) = self.codex_terminal_area else {
                return;
            };
            if let Some(pane) = self.codex.as_mut() {
                let batch = Self::mouse_scroll_batch_label(event_count);
                let terminal_point = Self::point_in_area(Some(area), mouse.column, mouse.row);
                let before = pane.scrollback;
                pane.scroll_lines(delta);
                let after = pane.scrollback;
                match Self::codex_terminal_scroll_route(before, after, terminal_point) {
                    CodexTerminalScrollRoute::Scrollback => {
                        self.codex_last_scroll_input = Some(format!(
                            "mouse {:?}{batch} -> codex {before}->{after}",
                            mouse.kind
                        ));
                        return;
                    }
                    CodexTerminalScrollRoute::None => {
                        self.codex_last_scroll_input = Some(format!(
                            "mouse {:?}{batch} -> codex {before}->{after} no-scroll",
                            mouse.kind
                        ));
                        return;
                    }
                }
            }
            return;
        }

        if Self::point_in_area(self.codex_status_area, mouse.column, mouse.row)
            || Self::point_in_area(self.codex_contract_area, mouse.column, mouse.row)
        {
            Self::scroll_document_offset(&mut self.codex_contract_scroll, delta);
            let batch = Self::mouse_scroll_batch_label(event_count);
            self.codex_last_scroll_input =
                Some(format!("mouse {:?}{batch} -> Addnessゴール", mouse.kind));
            return;
        }

        if Self::point_in_area(self.codex_activity_area, mouse.column, mouse.row) {
            Self::scroll_index(&mut self.codex_activity_scroll, delta);
            let batch = Self::mouse_scroll_batch_label(event_count);
            self.codex_last_scroll_input =
                Some(format!("mouse {:?}{batch} -> Addness更新", mouse.kind));
            return;
        }

        self.codex_last_scroll_input = Some(format!(
            "mouse {:?}{} at {},{} -> outside",
            mouse.kind,
            Self::mouse_scroll_batch_label(event_count),
            mouse.column,
            mouse.row
        ));
    }

    fn mouse_scroll_batch_label(event_count: usize) -> String {
        if event_count > 1 {
            format!(" x{event_count}")
        } else {
            String::new()
        }
    }

    fn mouse_scroll_delta(kind: MouseEventKind) -> Option<isize> {
        match kind {
            MouseEventKind::ScrollUp => Some(CODEX_WHEEL_LINES),
            MouseEventKind::ScrollDown => Some(-CODEX_WHEEL_LINES),
            MouseEventKind::ScrollLeft => Some(CODEX_WHEEL_LINES),
            MouseEventKind::ScrollRight => Some(-CODEX_WHEEL_LINES),
            _ => None,
        }
    }

    fn codex_terminal_scroll_route(
        before: usize,
        after: usize,
        terminal_point_inside: bool,
    ) -> CodexTerminalScrollRoute {
        if after != before {
            return CodexTerminalScrollRoute::Scrollback;
        }
        if !terminal_point_inside {
            return CodexTerminalScrollRoute::None;
        }
        CodexTerminalScrollRoute::None
    }

    fn scroll_index(offset: &mut usize, delta: isize) {
        if delta >= 0 {
            *offset = offset.saturating_add(delta as usize);
        } else {
            *offset = offset.saturating_sub((-delta) as usize);
        }
    }

    fn scroll_document_offset(offset: &mut usize, delta: isize) {
        Self::scroll_index(offset, -delta);
    }

    fn point_in_area(area: Option<Rect>, column: u16, row: u16) -> bool {
        let Some(area) = area else {
            return false;
        };
        let right = area.x.saturating_add(area.width);
        let bottom = area.y.saturating_add(area.height);
        column >= area.x && row >= area.y && column < right && row < bottom
    }

    #[cfg(test)]
    fn point_in_inner_area(area: Rect, column: u16, row: u16) -> Option<(u16, u16)> {
        let inner = Rect {
            x: area.x.saturating_add(1),
            y: area.y.saturating_add(1),
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };
        if inner.width == 0 || inner.height == 0 {
            return None;
        }
        let right = inner.x.saturating_add(inner.width);
        let bottom = inner.y.saturating_add(inner.height);
        if column < inner.x || row < inner.y || column >= right || row >= bottom {
            return None;
        }
        Some((column - inner.x, row - inner.y))
    }

    /// codex の作業差分（git diff --stat）をプリフィルして、対象ゴールへの
    /// 進捗コメントモーダルを開く（還流: コメント）。
    fn start_codex_reflow_comment(&mut self) {
        let Some(pane) = self.codex.as_ref() else {
            return;
        };
        let goal_id = pane.goal_id.clone();
        let goal_title = pane.goal_title.clone();
        let name = pane.kind().label();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let diff = agent::git_diff_stat(&cwd);
        let body = if diff.trim().is_empty() {
            String::new()
        } else {
            format!("{name}での作業差分:\n{diff}\n\n")
        };
        self.modal_state = Some(ModalState::AddComment {
            goal_id,
            goal_title,
            body,
        });
    }

    /// codex 対象ゴールの編集モーダルを開く（還流: ステータス）。
    /// ツリーのカーソルではなく `pane.goal_id` を対象にするため、ツリー再読込で
    /// カーソルがずれても正しいゴールを編集できる。
    fn start_codex_reflow_edit(&mut self) {
        let Some(goal_id) = self.codex.as_ref().map(|c| c.goal_id.clone()) else {
            return;
        };
        match self.api_call(self.client.get_goal(&goal_id)) {
            Ok(resp) => {
                let goal = resp.data;
                let current_status =
                    GoalDisplayStatus::from_goal_state(goal.status.as_ref(), goal.is_completed);
                let allowed_statuses = current_status.allowed_transitions();
                self.modal_state = Some(ModalState::EditGoal {
                    goal_id: goal.id,
                    title: goal.title,
                    description: goal.description.unwrap_or_default(),
                    current_status,
                    selected_status_index: 0,
                    allowed_statuses,
                    current_field: FormField::Title,
                });
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to load goal: {e}"));
            }
        }
    }

    /// codex 対象ゴールへの成果物追加モーダルを開く（還流: 成果物）。
    fn start_codex_reflow_deliverable(&mut self) {
        let Some((goal_id, goal_title)) = self
            .codex
            .as_ref()
            .map(|c| (c.goal_id.clone(), c.goal_title.clone()))
        else {
            return;
        };
        self.modal_state = Some(ModalState::AddDeliverable {
            goal_id,
            goal_title,
            kind: DeliverableKind::File,
            name: String::new(),
            value: String::new(),
            current_field: DeliverableFormField::Kind,
        });
    }

    fn reload_deliverables_for_goal(&mut self, goal_id: &str) {
        match self.api_call(self.client.get_goal_deliverables(goal_id)) {
            Ok(resp) => {
                let deliverables = resp.data.deliverables;
                self.goal_tree
                    .set_deliverables_for_goal_id(goal_id, deliverables.clone());
                self.todays_goals_tree
                    .set_deliverables_for_goal_id(goal_id, deliverables);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to reload deliverables: {e}"));
            }
        }
    }

    fn reload_comments_for_goal(&mut self, goal_id: &str) {
        match self.api_call(self.client.list_comments(goal_id)) {
            Ok(resp) => {
                let comments = resp.comments;
                self.goal_tree
                    .set_comments_for_goal_id(goal_id, comments.clone());
                self.todays_goals_tree
                    .set_comments_for_goal_id(goal_id, comments);
                // 削除・解決で行数が減るとカーソルが範囲外/別ノードを指すため再クランプ。
                self.goal_tree.clamp_cursor();
                self.todays_goals_tree.clamp_cursor();
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to reload comments: {e}"));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Comment actions
    // -----------------------------------------------------------------------

    fn start_add_comment_modal(&mut self) {
        let Some((goal_id, goal_title)) = self.selected_goal_context() else {
            self.error_message = Some("Please select a goal to add a comment".to_string());
            return;
        };
        self.modal_state = Some(ModalState::AddComment {
            goal_id,
            goal_title,
            body: String::new(),
        });
    }

    fn start_reply_comment_modal(&mut self) {
        let Some((goal_id, comment_id, content, _)) = self.selected_comment_context() else {
            self.error_message = Some("Please select a comment to reply to".to_string());
            return;
        };
        self.modal_state = Some(ModalState::ReplyComment {
            goal_id,
            parent_comment_id: comment_id,
            parent_excerpt: truncate_comment(&content),
            body: String::new(),
        });
    }

    fn start_edit_comment_modal(&mut self) {
        let Some((goal_id, comment_id, content, _)) = self.selected_comment_context() else {
            self.error_message = Some("Please select a comment to edit".to_string());
            return;
        };
        self.modal_state = Some(ModalState::EditComment {
            goal_id,
            comment_id,
            body: content,
        });
    }

    fn start_delete_comment_modal(&mut self) {
        let Some((goal_id, comment_id, content, _)) = self.selected_comment_context() else {
            self.error_message = Some("Please select a comment to delete".to_string());
            return;
        };
        if self.allow_delete_comment_without_confirm {
            self.modal_submit_delete_comment(goal_id, comment_id);
            return;
        }
        self.modal_state = Some(ModalState::DeleteComment {
            goal_id,
            comment_id,
            excerpt: truncate_comment(&content),
            confirm_index: CONFIRM_CANCEL,
        });
    }

    fn start_react_comment_modal(&mut self) {
        let Some((goal_id, comment_id, _, _)) = self.selected_comment_context() else {
            self.error_message = Some("Please select a comment to react to".to_string());
            return;
        };
        self.modal_state = Some(ModalState::ReactComment {
            goal_id,
            comment_id,
            emojis: vec!["👍", "❤️", "🎉", "👀", "🙏", "🚀"],
            selected_index: 0,
        });
    }

    fn do_set_comment_resolved(&mut self, resolved: bool) {
        let Some((goal_id, comment_id, _, _)) = self.selected_comment_context() else {
            self.error_message = Some("Please select a comment".to_string());
            return;
        };
        let result = if resolved {
            self.api_call(self.client.resolve_comment(&comment_id))
                .map(|_| ())
        } else {
            self.api_call(self.client.unresolve_comment(&comment_id))
                .map(|_| ())
        };
        match result {
            Ok(()) => {
                self.success_message = Some(
                    if resolved {
                        "Comment resolved"
                    } else {
                        "Comment unresolved"
                    }
                    .to_string(),
                );
                self.reload_comments_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to update comment: {e}"));
            }
        }
    }

    fn modal_submit_add_comment(
        &mut self,
        goal_id: String,
        parent_comment_id: Option<String>,
        body: String,
    ) {
        let body = body.trim();
        if body.is_empty() {
            self.error_message = Some("Comment body is required".to_string());
            return;
        }
        // AIではなく人間の操作だが、CLI経由と区別できるよう本文はそのまま送る。
        let result = self.api_call(self.client.create_comment_with_options(
            &goal_id,
            body,
            parent_comment_id,
            Vec::new(),
        ));
        match result {
            Ok(_) => {
                self.success_message = Some("Comment posted".to_string());
                self.reload_comments_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to post comment: {e}"));
            }
        }
    }

    fn modal_submit_edit_comment(&mut self, goal_id: String, comment_id: String, body: String) {
        let body = body.trim();
        if body.is_empty() {
            self.error_message = Some("Comment body is required".to_string());
            return;
        }
        match self.api_call(self.client.update_comment(&comment_id, body, Vec::new())) {
            Ok(_) => {
                self.success_message = Some("Comment updated".to_string());
                self.reload_comments_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to update comment: {e}"));
            }
        }
    }

    fn modal_submit_delete_comment(&mut self, goal_id: String, comment_id: String) {
        match self.api_call(self.client.delete_comment(&comment_id)) {
            Ok(()) => {
                self.success_message = Some("Comment deleted".to_string());
                self.reload_comments_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to delete comment: {e}"));
            }
        }
    }

    fn modal_submit_react_comment(&mut self, goal_id: String, comment_id: String, emoji: &str) {
        match self.api_call(self.client.add_reaction(&comment_id, emoji)) {
            Ok(()) => {
                self.success_message = Some(format!("Reacted {emoji}"));
                self.reload_comments_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to react: {e}"));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Event handling
    // -----------------------------------------------------------------------

    fn handle_events(&mut self) -> Result<()> {
        let event = event::read()?;
        self.handle_event(event)
    }

    fn handle_event(&mut self, event: Event) -> Result<()> {
        // 端末リサイズ時はレイアウトが変わり前サイズの残像が出るため、全クリアを予約する。
        if let Event::Resize(_, _) = event {
            self.needs_full_clear = true;
            return Ok(());
        }

        if let Event::Mouse(me) = event {
            // codex 使用中のみ有効なマウスキャプチャを、右側の会話領域に限定して扱う。
            self.handle_codex_mouse_batch(me)?;
            return Ok(());
        }

        if let Event::Paste(text) = event {
            if self.active_pane == ActivePane::Codex
                && self.modal_state.is_none()
                && !self.show_help
            {
                self.handle_codex_paste(&text);
            } else if self.modal_state.is_some() && !self.show_help {
                self.handle_modal_paste(&text);
            }
            return Ok(());
        }

        if let Event::Key(key) = event {
            if key.kind != KeyEventKind::Press {
                return Ok(());
            }

            if self.active_pane == ActivePane::Codex
                && self.modal_state.is_none()
                && Self::is_ctrl_help_key(key)
            {
                self.show_help = !self.show_help;
                self.help_scroll = 0;
                return Ok(());
            }
            if Self::is_ctrl_help_key(key) {
                return Ok(());
            }

            if self.show_help {
                // ヘルプ表示中は閉じる操作とスクロールのみ受け付ける。
                // 実際の最大スクロール量は描画側（行数・枠高さ）が知っているため、
                // ここでは意図だけを反映し、描画側で毎フレームクランプし直す。
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                        self.show_help = false;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.help_scroll = self.help_scroll.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.help_scroll = self.help_scroll.saturating_add(1);
                    }
                    KeyCode::PageUp => {
                        self.help_scroll = self.help_scroll.saturating_sub(10);
                    }
                    KeyCode::PageDown => {
                        self.help_scroll = self.help_scroll.saturating_add(10);
                    }
                    KeyCode::Home => {
                        self.help_scroll = 0;
                    }
                    KeyCode::End => {
                        // 描画側で実際の最大値へクランプされる。
                        self.help_scroll = usize::MAX / 2;
                    }
                    _ => {}
                }
                return Ok(());
            }

            // codex ペインにフォーカス中はキーを codex へ転送する。
            // ただし還流モーダルが開いている間はモーダル入力を優先する。
            if self.active_pane == ActivePane::Codex && self.modal_state.is_none() {
                self.handle_codex_key(key);
                return Ok(());
            }

            // ステータスメッセージはキー入力では消さず、時間経過で自動クリアする
            // （run ループの expire_status_messages が担当）。新しいメッセージが
            // 来れば下の各ハンドラが上書きし、期限も張り直される。

            if self.modal_state.is_some() {
                self.handle_modal_input(key);
            } else if self.show_org_popup {
                self.handle_org_popup(key.code);
            } else {
                self.handle_normal(key.code);
            }
        }
        Ok(())
    }

    fn is_ctrl_help_key(key: KeyEvent) -> bool {
        key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q'))
    }

    fn handle_org_popup(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.show_org_popup = false;
            }
            KeyCode::Up | KeyCode::Char('k') if self.org_popup_index > 0 => {
                self.org_popup_index -= 1;
            }
            KeyCode::Down | KeyCode::Char('j')
                if !self.orgs.is_empty() && self.org_popup_index < self.orgs.len() - 1 =>
            {
                self.org_popup_index += 1;
            }
            KeyCode::Enter => {
                let new_index = self.org_popup_index;
                self.show_org_popup = false;
                if new_index != self.current_org_index {
                    self.current_org_index = new_index;
                    self.load_org_scoped_data();
                    // Execution は次に表示された時に再ロードする。
                    self.todays_loaded = false;
                    self.todays_goals_tree = GoalTree::empty();
                }
            }
            _ => {}
        }
    }

    fn handle_normal(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('?') => {
                self.show_help = true;
                self.help_scroll = 0;
            }
            KeyCode::Char('q') | KeyCode::Esc => self.running = false,
            KeyCode::Tab => {
                self.active_pane = match self.active_pane {
                    ActivePane::OrgSelector => ActivePane::Navigation,
                    ActivePane::Navigation => ActivePane::Content,
                    ActivePane::Content | ActivePane::Codex => ActivePane::OrgSelector,
                };
            }
            KeyCode::BackTab => {
                self.active_pane = match self.active_pane {
                    ActivePane::OrgSelector => ActivePane::Content,
                    ActivePane::Navigation => ActivePane::OrgSelector,
                    ActivePane::Content | ActivePane::Codex => ActivePane::Navigation,
                };
            }
            KeyCode::Enter => match self.active_pane {
                ActivePane::OrgSelector => {
                    self.org_popup_index = self.current_org_index;
                    self.show_org_popup = true;
                }
                ActivePane::Navigation => {
                    self.active_pane = ActivePane::Content;
                }
                ActivePane::Content if self.sidebar_index == 0 || self.sidebar_index == 1 => {
                    self.handle_goal_expand();
                }
                _ => {}
            },
            KeyCode::Char('o') | KeyCode::Char(' ')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_action_menu();
            }
            KeyCode::Up | KeyCode::Char('k') => match self.active_pane {
                ActivePane::Navigation if self.sidebar_index > 0 => {
                    self.sidebar_index -= 1;
                }
                ActivePane::Content if self.sidebar_index == 0 || self.sidebar_index == 1 => {
                    self.active_goal_tree_mut().cursor_up();
                }
                ActivePane::Content if self.sidebar_index == 2 => {
                    self.members_cursor_up();
                }
                _ => {}
            },
            KeyCode::Down | KeyCode::Char('j') => match self.active_pane {
                ActivePane::Navigation if self.sidebar_index < self.sidebar_items.len() - 1 => {
                    self.sidebar_index += 1;
                }
                ActivePane::Content if self.sidebar_index == 0 || self.sidebar_index == 1 => {
                    self.active_goal_tree_mut().cursor_down();
                }
                ActivePane::Content if self.sidebar_index == 2 => {
                    self.members_cursor_down();
                }
                _ => {}
            },
            KeyCode::Right | KeyCode::Char('l') => match self.active_pane {
                ActivePane::Navigation => {
                    self.active_pane = ActivePane::Content;
                }
                ActivePane::Content if self.sidebar_index == 0 || self.sidebar_index == 1 => {
                    self.handle_goal_expand();
                }
                _ => {}
            },
            KeyCode::Left | KeyCode::Char('h')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.active_goal_tree_mut().collapse_or_parent();
            }
            KeyCode::Char('c')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_create_goal_modal();
            }
            KeyCode::Char('C')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.active_goal_tree_mut().cycle_comment_view();
                let mode = self.active_goal_tree().comment_view.label();
                self.success_message = Some(format!("Comments: {mode}"));
            }
            KeyCode::Char('e')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_edit_goal_modal();
            }
            KeyCode::Char('d')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_delete_goal_modal();
            }
            KeyCode::Char('a')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_add_deliverable_modal();
            }
            KeyCode::Char('u')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_update_deliverable_modal();
            }
            KeyCode::Char('r')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_rename_deliverable_modal();
            }
            KeyCode::Char('m')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_move_deliverable_modal();
            }
            KeyCode::Char('x')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_delete_deliverable_modal();
            }
            _ => {}
        }
    }

    fn handle_goal_expand(&mut self) {
        // Check if this is a complete tree (from todays-goals) - if so, only toggle expand
        let is_completed_tree = !self.active_goal_tree().is_required_to_fetch;

        // Check if cursor is on CommentOmitted row - if so, increase display limit
        {
            let tree = self.active_goal_tree();
            let rows = tree.flatten();
            if matches!(rows.get(tree.cursor), Some(TreeRow::CommentOmitted { .. })) {
                self.active_goal_tree_mut().increase_comment_limit();
                return;
            }
        }

        // If this is a complete tree, just toggle expand without fetching
        if is_completed_tree {
            self.active_goal_tree_mut().toggle_expand();
            return;
        }

        // Extract info from the cursor row before mutating
        let info = {
            let tree = self.active_goal_tree();
            let rows = tree.flatten();
            match rows.get(tree.cursor) {
                Some(TreeRow::Goal {
                    goal_id,
                    has_children,
                    children_loaded,
                    comments_loaded,
                    deliverables_loaded,
                    ..
                }) => Some((
                    goal_id.to_string(),
                    *has_children,
                    *children_loaded,
                    *comments_loaded,
                    *deliverables_loaded,
                )),
                _ => None,
            }
        };

        let Some((goal_id, has_children, children_loaded, comments_loaded, deliverables_loaded)) =
            info
        else {
            return;
        };

        // Determine what needs to be loaded
        let need_children = has_children && !children_loaded;
        let need_comments = !comments_loaded;
        let need_deliverables = !deliverables_loaded;

        // Lazy-load data if needed
        if need_comments || need_deliverables || need_children {
            let goal_id_ref = &goal_id;
            let client = &self.client;
            let result = self.api_call(async {
                tokio::try_join!(
                    async {
                        if need_comments {
                            client.list_comments(goal_id_ref).await
                        } else {
                            Ok(crate::api::CommentsResponse {
                                comments: vec![],
                                total_count: 0,
                            })
                        }
                    },
                    async {
                        if need_deliverables {
                            client.get_goal_deliverables(goal_id_ref).await
                        } else {
                            Ok(crate::api::ApiResponse {
                                data: crate::api::DeliverableListData {
                                    deliverables: vec![],
                                    total: 0,
                                },
                            })
                        }
                    },
                    async {
                        if need_children {
                            client.get_goal_children(goal_id_ref, 100, 0).await
                        } else {
                            Ok(crate::api::ApiResponse {
                                data: crate::api::GoalChildrenData {
                                    children: vec![],
                                    pagination: None,
                                },
                            })
                        }
                    }
                )
            });

            match result {
                Ok((comments_resp, deliverables_resp, children_resp)) => {
                    let tree = self.active_goal_tree_mut();
                    if need_comments {
                        tree.set_comments_at_cursor(comments_resp.comments);
                    }
                    if need_deliverables {
                        tree.set_deliverables_at_cursor(deliverables_resp.data.deliverables);
                    }
                    if need_children {
                        tree.set_children_at_cursor(children_resp.data.children);
                    }
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to load data: {e}"));
                    return;
                }
            }
        }

        self.active_goal_tree_mut().toggle_expand();
    }

    // -----------------------------------------------------------------------
    // Modal handling - Create/Edit Goal
    // -----------------------------------------------------------------------

    fn start_create_goal_modal(&mut self) {
        // Get parent goal info from cursor position
        let tree = self.active_goal_tree();
        let rows = tree.flatten();
        let (parent_goal_id, parent_goal_title) = match rows.get(tree.cursor) {
            Some(TreeRow::Goal { goal_id, title, .. }) => {
                (Some(goal_id.to_string()), Some(title.to_string()))
            }
            _ => (None, None),
        };

        self.modal_state = Some(ModalState::CreateGoal {
            title: String::new(),
            description: String::new(),
            parent_goal_id,
            parent_goal_title,
            current_field: FormField::Title,
        });
    }

    fn start_edit_goal_modal(&mut self) {
        // Get goal info from cursor position
        let tree = self.active_goal_tree();
        let rows = tree.flatten();
        let goal_info = match rows.get(tree.cursor) {
            Some(TreeRow::Goal {
                goal_id,
                status,
                is_completed,
                ..
            }) => {
                let display_status = GoalDisplayStatus::from_goal_state(*status, *is_completed);
                Some((goal_id.to_string(), display_status))
            }
            _ => None,
        };

        if let Some((goal_id, current_status)) = goal_info {
            // Fetch full goal details to get description
            match self.api_call(self.client.get_goal(&goal_id)) {
                Ok(resp) => {
                    let goal = resp.data;
                    let allowed_statuses = current_status.allowed_transitions();

                    self.modal_state = Some(ModalState::EditGoal {
                        goal_id: goal.id,
                        title: goal.title,
                        description: goal.description.unwrap_or_default(),
                        current_status,
                        selected_status_index: 0,
                        allowed_statuses,
                        current_field: FormField::Title,
                    });
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to load goal: {e}"));
                }
            }
        } else {
            self.error_message = Some("Please select a goal to edit".to_string());
        }
    }

    fn start_delete_goal_modal(&mut self) {
        // Get goal info from cursor position
        let tree = self.active_goal_tree();
        let rows = tree.flatten();
        let goal_info = match rows.get(tree.cursor) {
            Some(TreeRow::Goal { goal_id, title, .. }) => {
                Some((goal_id.to_string(), title.to_string()))
            }
            _ => None,
        };

        if let Some((goal_id, goal_title)) = goal_info {
            if self.allow_delete_goal_without_confirm {
                self.modal_submit_delete(goal_id);
                return;
            }
            self.modal_state = Some(ModalState::DeleteGoal {
                goal_id,
                goal_title,
                confirm_index: CONFIRM_CANCEL,
            });
        } else {
            self.error_message = Some("Please select a goal to delete".to_string());
        }
    }

    fn start_add_deliverable_modal(&mut self) {
        let Some((goal_id, goal_title)) = self.selected_goal_context() else {
            self.error_message = Some("Please select a goal to add a deliverable".to_string());
            return;
        };

        self.modal_state = Some(ModalState::AddDeliverable {
            goal_id,
            goal_title,
            kind: DeliverableKind::File,
            name: String::new(),
            value: String::new(),
            current_field: DeliverableFormField::Kind,
        });
    }

    fn start_update_deliverable_modal(&mut self) {
        let Some((goal_id, deliverable_id, deliverable_name, _, node_type)) =
            self.selected_deliverable_context()
        else {
            self.error_message = Some("Please select a document deliverable to update".to_string());
            return;
        };
        if node_type != DeliverableType::Document {
            self.error_message = Some("Only document deliverables can be updated".to_string());
            return;
        }

        self.modal_state = Some(ModalState::UpdateDeliverable {
            goal_id,
            deliverable_id,
            deliverable_name,
            content_file: String::new(),
        });
    }

    fn start_rename_deliverable_modal(&mut self) {
        let Some((goal_id, deliverable_id, deliverable_name, _, _)) =
            self.selected_deliverable_context()
        else {
            self.error_message = Some("Please select a deliverable to rename".to_string());
            return;
        };

        self.modal_state = Some(ModalState::RenameDeliverable {
            goal_id,
            deliverable_id,
            current_name: deliverable_name.clone(),
            name: deliverable_name,
        });
    }

    fn start_move_deliverable_modal(&mut self) {
        let Some((goal_id, deliverable_id, deliverable_name, _, _)) =
            self.selected_deliverable_context()
        else {
            self.error_message = Some("Please select a deliverable to move".to_string());
            return;
        };
        let targets = self.deliverable_folder_targets_for_goal(&goal_id, &deliverable_id);

        self.modal_state = Some(ModalState::MoveDeliverable {
            goal_id,
            deliverable_id,
            deliverable_name,
            targets,
            selected_index: 0,
        });
    }

    fn start_delete_deliverable_modal(&mut self) {
        let Some((goal_id, deliverable_id, deliverable_name, _, _)) =
            self.selected_deliverable_context()
        else {
            self.error_message = Some("Please select a deliverable to delete".to_string());
            return;
        };

        if self.allow_delete_deliverable_without_confirm {
            self.modal_submit_delete_deliverable(goal_id, deliverable_id);
            return;
        }
        self.modal_state = Some(ModalState::DeleteDeliverable {
            goal_id,
            deliverable_id,
            deliverable_name,
            confirm_index: CONFIRM_CANCEL,
        });
    }

    // -----------------------------------------------------------------------
    // File path entry helpers (Tab completion + file picker)
    // -----------------------------------------------------------------------

    /// 現在フォーカス中のフィールドがファイルパス入力なら、その可変参照を返す。
    fn current_path_field_mut(&mut self) -> Option<&mut String> {
        match &mut self.modal_state {
            Some(ModalState::AddDeliverable {
                kind,
                value,
                current_field,
                ..
            }) if *current_field == DeliverableFormField::Value
                && matches!(kind, DeliverableKind::File | DeliverableKind::Document) =>
            {
                Some(value)
            }
            Some(ModalState::UpdateDeliverable { content_file, .. }) => Some(content_file),
            _ => None,
        }
    }

    /// パス欄で補完が前進したら適用して true（Tabを消費）。
    /// パス欄でない、または補完で文字列が変わらなかった場合は false を返し、
    /// 呼び出し側で通常のフィールド送り（Tab）にフォールバックさせる。
    /// これにより最後尾の Value 欄でも Tab が無反応にならず、次フィールドへ回れる。
    fn try_complete_path_field(&mut self) -> bool {
        let Some(field) = self.current_path_field_mut() else {
            return false;
        };
        match complete_path(field) {
            Some(completed) if completed != *field => {
                *field = completed;
                true
            }
            _ => false,
        }
    }

    /// 現在のパス欄の値から開始ディレクトリを決めてファイラーを開く。
    fn open_file_picker(&mut self) {
        let ret = match &self.modal_state {
            // ファイラーが書き戻すのはファイルパス欄（File/Document）のみ。
            // Link/Folder の value はパスではないため対象外にする。
            Some(ModalState::AddDeliverable {
                goal_id,
                goal_title,
                kind,
                name,
                value,
                ..
            }) if matches!(kind, DeliverableKind::File | DeliverableKind::Document) => {
                FilePickerReturn::AddDeliverable {
                    goal_id: goal_id.clone(),
                    goal_title: goal_title.clone(),
                    kind: kind.clone(),
                    name: name.clone(),
                    value: value.clone(),
                }
            }
            Some(ModalState::UpdateDeliverable {
                goal_id,
                deliverable_id,
                deliverable_name,
                content_file,
            }) => FilePickerReturn::UpdateDeliverable {
                goal_id: goal_id.clone(),
                deliverable_id: deliverable_id.clone(),
                deliverable_name: deliverable_name.clone(),
                content_file: content_file.clone(),
            },
            _ => return,
        };

        let current = match &ret {
            FilePickerReturn::AddDeliverable { value, .. } => value.clone(),
            FilePickerReturn::UpdateDeliverable { content_file, .. } => content_file.clone(),
        };
        let dir = initial_picker_dir(&current);
        let entries = read_dir_entries(&dir);
        self.modal_state = Some(ModalState::FilePicker {
            dir,
            entries,
            selected_index: 0,
            ret,
        });
    }

    /// 選んだパスを ret の元モーダルに書き戻す（path=Some で確定、None でキャンセル復元）。
    fn close_file_picker(&mut self, picked: Option<String>) {
        let Some(ModalState::FilePicker { ret, .. }) = self.modal_state.take() else {
            return;
        };
        self.modal_state = Some(match ret {
            FilePickerReturn::AddDeliverable {
                goal_id,
                goal_title,
                kind,
                name,
                value,
            } => ModalState::AddDeliverable {
                goal_id,
                goal_title,
                kind,
                name,
                value: picked.unwrap_or(value),
                current_field: DeliverableFormField::Value,
            },
            FilePickerReturn::UpdateDeliverable {
                goal_id,
                deliverable_id,
                deliverable_name,
                content_file,
            } => ModalState::UpdateDeliverable {
                goal_id,
                deliverable_id,
                deliverable_name,
                content_file: picked.unwrap_or(content_file),
            },
        });
    }

    fn handle_file_picker_input(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.close_file_picker(None),
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(ModalState::FilePicker {
                    selected_index,
                    entries,
                    ..
                }) = &mut self.modal_state
                    && !entries.is_empty()
                {
                    *selected_index = selected_index.saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(ModalState::FilePicker {
                    selected_index,
                    entries,
                    ..
                }) = &mut self.modal_state
                    && *selected_index + 1 < entries.len()
                {
                    *selected_index += 1;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => self.file_picker_go_parent(),
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => self.file_picker_activate(),
            _ => {}
        }
    }

    /// 親ディレクトリへ移動する。
    fn file_picker_go_parent(&mut self) {
        if let Some(ModalState::FilePicker {
            dir,
            entries,
            selected_index,
            ..
        }) = &mut self.modal_state
            && let Some(parent) = dir.parent()
        {
            *dir = parent.to_path_buf();
            *entries = read_dir_entries(dir);
            *selected_index = 0;
        }
    }

    /// 選択中のエントリがディレクトリなら入る、ファイルなら選択して書き戻す。
    fn file_picker_activate(&mut self) {
        let Some(ModalState::FilePicker {
            dir,
            entries,
            selected_index,
            ..
        }) = &self.modal_state
        else {
            return;
        };
        let Some(entry) = entries.get(*selected_index) else {
            return;
        };
        let path = dir.join(&entry.name);
        if entry.is_dir {
            if let Some(ModalState::FilePicker {
                dir,
                entries,
                selected_index,
                ..
            }) = &mut self.modal_state
            {
                *dir = path;
                *entries = read_dir_entries(dir);
                *selected_index = 0;
            }
        } else {
            self.close_file_picker(Some(path.to_string_lossy().into_owned()));
        }
    }

    fn handle_modal_input(&mut self, key: KeyEvent) {
        // ファイラーは専用のキー操作で処理する。
        if matches!(self.modal_state, Some(ModalState::FilePicker { .. })) {
            self.handle_file_picker_input(key.code);
            return;
        }

        // Ctrl+F: パス欄からファイラーを開く。
        if key.code == KeyCode::Char('f')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && self.current_path_field_mut().is_some()
        {
            self.open_file_picker();
            return;
        }

        let code = key.code;
        match code {
            KeyCode::Esc => {
                self.modal_state = None;
            }
            KeyCode::Tab => {
                // パス欄ではTabでファイルシステムからパス補完する。
                if !self.try_complete_path_field() {
                    self.modal_next_field();
                }
            }
            KeyCode::BackTab => {
                self.modal_prev_field();
            }
            KeyCode::Enter => {
                self.modal_submit();
            }
            KeyCode::Up => {
                if matches!(self.modal_state, Some(ModalState::ActionMenu { .. })) {
                    self.modal_menu_prev();
                } else if matches!(self.modal_state, Some(ModalState::MoveDeliverable { .. })) {
                    self.modal_move_target_prev();
                } else if matches!(self.modal_state, Some(ModalState::ReactComment { .. })) {
                    self.modal_react_prev();
                } else if let Some(ModalState::EditGoal {
                    current_field: FormField::Status,
                    ..
                }) = &self.modal_state
                {
                    self.modal_prev_status();
                } else if let Some(ModalState::AddDeliverable {
                    current_field: DeliverableFormField::Kind,
                    ..
                }) = &self.modal_state
                {
                    self.modal_prev_deliverable_kind();
                }
            }
            KeyCode::Down => {
                if matches!(self.modal_state, Some(ModalState::ActionMenu { .. })) {
                    self.modal_menu_next();
                } else if matches!(self.modal_state, Some(ModalState::MoveDeliverable { .. })) {
                    self.modal_move_target_next();
                } else if matches!(self.modal_state, Some(ModalState::ReactComment { .. })) {
                    self.modal_react_next();
                } else if let Some(ModalState::EditGoal {
                    current_field: FormField::Status,
                    ..
                }) = &self.modal_state
                {
                    self.modal_next_status();
                } else if let Some(ModalState::AddDeliverable {
                    current_field: DeliverableFormField::Kind,
                    ..
                }) = &self.modal_state
                {
                    self.modal_next_deliverable_kind();
                }
            }
            KeyCode::Left => {
                if !self.modal_confirm_prev()
                    && matches!(self.modal_state, Some(ModalState::ReactComment { .. }))
                {
                    self.modal_react_prev();
                }
            }
            KeyCode::Right => {
                if !self.modal_confirm_next()
                    && matches!(self.modal_state, Some(ModalState::ReactComment { .. }))
                {
                    self.modal_react_next();
                }
            }
            KeyCode::Char(c) => {
                if matches!(self.modal_state, Some(ModalState::ActionMenu { .. })) {
                    match c {
                        'j' => self.modal_menu_next(),
                        'k' => self.modal_menu_prev(),
                        _ => {}
                    }
                } else if matches!(self.modal_state, Some(ModalState::MoveDeliverable { .. })) {
                    match c {
                        'j' => self.modal_move_target_next(),
                        'k' => self.modal_move_target_prev(),
                        _ => {}
                    }
                } else if self.modal_confirm_key(c) {
                } else if matches!(self.modal_state, Some(ModalState::ReactComment { .. })) {
                    match c {
                        'j' | 'l' => self.modal_react_next(),
                        'k' | 'h' => self.modal_react_prev(),
                        _ => {}
                    }
                } else {
                    self.modal_input_char(c);
                }
            }
            KeyCode::Backspace => {
                self.modal_backspace();
            }
            _ => {}
        }
    }

    fn modal_confirm_index_mut(&mut self) -> Option<&mut usize> {
        match &mut self.modal_state {
            Some(ModalState::DeleteGoal { confirm_index, .. })
            | Some(ModalState::DeleteDeliverable { confirm_index, .. })
            | Some(ModalState::DeleteComment { confirm_index, .. }) => Some(confirm_index),
            _ => None,
        }
    }

    fn modal_confirm_prev(&mut self) -> bool {
        let Some(index) = self.modal_confirm_index_mut() else {
            return false;
        };
        *index = if *index == CONFIRM_CANCEL {
            CONFIRM_ALWAYS
        } else {
            (*index).saturating_sub(1)
        };
        true
    }

    fn modal_confirm_next(&mut self) -> bool {
        let Some(index) = self.modal_confirm_index_mut() else {
            return false;
        };
        *index = (*index + 1) % CONFIRM_CHOICE_COUNT;
        true
    }

    fn modal_confirm_key(&mut self, c: char) -> bool {
        let Some(index) = self.modal_confirm_index_mut() else {
            return false;
        };
        match c.to_ascii_lowercase() {
            'h' | 'n' => *index = CONFIRM_CANCEL,
            'l' | 'y' => *index = CONFIRM_APPLY,
            'a' => *index = CONFIRM_ALWAYS,
            _ => return false,
        }
        true
    }

    fn modal_next_field(&mut self) {
        if let Some(ref mut modal) = self.modal_state {
            match modal {
                ModalState::CreateGoal { current_field, .. } => {
                    *current_field = match current_field {
                        FormField::Title => FormField::Description,
                        FormField::Description => FormField::Title,
                        _ => FormField::Title,
                    };
                }
                ModalState::EditGoal { current_field, .. } => {
                    *current_field = match current_field {
                        FormField::Title => FormField::Description,
                        FormField::Description => FormField::Status,
                        FormField::Status => FormField::Title,
                    };
                }
                ModalState::DeleteGoal { .. } => {
                    // No fields to navigate in delete modal
                }
                ModalState::AddDeliverable { current_field, .. } => {
                    *current_field = match current_field {
                        DeliverableFormField::Kind => DeliverableFormField::Name,
                        DeliverableFormField::Name => DeliverableFormField::Value,
                        DeliverableFormField::Value => DeliverableFormField::Kind,
                    };
                }
                ModalState::ActionMenu { .. } | ModalState::MoveDeliverable { .. } => {}
                ModalState::UpdateDeliverable { .. }
                | ModalState::RenameDeliverable { .. }
                | ModalState::DeleteDeliverable { .. } => {}
                ModalState::AddComment { .. }
                | ModalState::ReplyComment { .. }
                | ModalState::EditComment { .. }
                | ModalState::DeleteComment { .. }
                | ModalState::ReactComment { .. }
                | ModalState::FilePicker { .. } => {}
            }
        }
    }

    fn modal_prev_field(&mut self) {
        if let Some(ref mut modal) = self.modal_state {
            match modal {
                ModalState::CreateGoal { current_field, .. } => {
                    *current_field = match current_field {
                        FormField::Title => FormField::Description,
                        FormField::Description => FormField::Title,
                        _ => FormField::Title,
                    };
                }
                ModalState::EditGoal { current_field, .. } => {
                    *current_field = match current_field {
                        FormField::Title => FormField::Status,
                        FormField::Description => FormField::Title,
                        FormField::Status => FormField::Description,
                    };
                }
                ModalState::DeleteGoal { .. } => {
                    // No fields to navigate in delete modal
                }
                ModalState::AddDeliverable { current_field, .. } => {
                    *current_field = match current_field {
                        DeliverableFormField::Kind => DeliverableFormField::Value,
                        DeliverableFormField::Name => DeliverableFormField::Kind,
                        DeliverableFormField::Value => DeliverableFormField::Name,
                    };
                }
                ModalState::ActionMenu { .. } | ModalState::MoveDeliverable { .. } => {}
                ModalState::UpdateDeliverable { .. }
                | ModalState::RenameDeliverable { .. }
                | ModalState::DeleteDeliverable { .. } => {}
                ModalState::AddComment { .. }
                | ModalState::ReplyComment { .. }
                | ModalState::EditComment { .. }
                | ModalState::DeleteComment { .. }
                | ModalState::ReactComment { .. }
                | ModalState::FilePicker { .. } => {}
            }
        }
    }

    fn modal_input_char(&mut self, c: char) {
        if let Some(ref mut modal) = self.modal_state {
            match modal {
                ModalState::CreateGoal {
                    title,
                    description,
                    current_field,
                    ..
                } => match current_field {
                    FormField::Title => title.push(c),
                    FormField::Description => description.push(c),
                    _ => {}
                },
                ModalState::EditGoal {
                    title,
                    description,
                    current_field,
                    ..
                } => match current_field {
                    FormField::Title => title.push(c),
                    FormField::Description => description.push(c),
                    _ => {}
                },
                ModalState::DeleteGoal { .. } => {
                    // No text input in delete modal
                }
                ModalState::AddDeliverable {
                    name,
                    value,
                    current_field,
                    ..
                } => match current_field {
                    DeliverableFormField::Name => name.push(c),
                    DeliverableFormField::Value => value.push(c),
                    _ => {}
                },
                ModalState::UpdateDeliverable { content_file, .. } => {
                    content_file.push(c);
                }
                ModalState::RenameDeliverable { name, .. } => {
                    name.push(c);
                }
                ModalState::AddComment { body, .. }
                | ModalState::ReplyComment { body, .. }
                | ModalState::EditComment { body, .. } => {
                    body.push(c);
                }
                ModalState::ActionMenu { .. } | ModalState::MoveDeliverable { .. } => {}
                ModalState::DeleteDeliverable { .. }
                | ModalState::DeleteComment { .. }
                | ModalState::ReactComment { .. }
                | ModalState::FilePicker { .. } => {}
            }
        }
    }

    fn modal_backspace(&mut self) {
        if let Some(ref mut modal) = self.modal_state {
            match modal {
                ModalState::CreateGoal {
                    title,
                    description,
                    current_field,
                    ..
                } => match current_field {
                    FormField::Title => {
                        title.pop();
                    }
                    FormField::Description => {
                        description.pop();
                    }
                    _ => {}
                },
                ModalState::EditGoal {
                    title,
                    description,
                    current_field,
                    ..
                } => match current_field {
                    FormField::Title => {
                        title.pop();
                    }
                    FormField::Description => {
                        description.pop();
                    }
                    _ => {}
                },
                ModalState::DeleteGoal { .. } => {
                    // No text input in delete modal
                }
                ModalState::AddDeliverable {
                    name,
                    value,
                    current_field,
                    ..
                } => match current_field {
                    DeliverableFormField::Name => {
                        name.pop();
                    }
                    DeliverableFormField::Value => {
                        value.pop();
                    }
                    _ => {}
                },
                ModalState::UpdateDeliverable { content_file, .. } => {
                    content_file.pop();
                }
                ModalState::RenameDeliverable { name, .. } => {
                    name.pop();
                }
                ModalState::AddComment { body, .. }
                | ModalState::ReplyComment { body, .. }
                | ModalState::EditComment { body, .. } => {
                    body.pop();
                }
                ModalState::ActionMenu { .. } | ModalState::MoveDeliverable { .. } => {}
                ModalState::DeleteDeliverable { .. }
                | ModalState::DeleteComment { .. }
                | ModalState::ReactComment { .. }
                | ModalState::FilePicker { .. } => {}
            }
        }
    }

    fn modal_submit(&mut self) {
        match self.modal_state.take() {
            Some(ModalState::ActionMenu {
                items,
                selected_index,
                ..
            }) => {
                if let Some(item) = items.get(selected_index).cloned() {
                    self.run_action_menu_item(item);
                }
            }
            Some(ModalState::CreateGoal {
                title,
                description,
                parent_goal_id,
                ..
            }) => {
                self.modal_submit_create(title, description, parent_goal_id);
            }
            Some(ModalState::EditGoal {
                goal_id,
                title,
                description,
                current_status,
                selected_status_index,
                allowed_statuses,
                ..
            }) => {
                // Get selected status from allowed transitions
                let new_status = allowed_statuses
                    .get(selected_status_index)
                    .cloned()
                    .unwrap_or(current_status);
                self.modal_submit_edit(goal_id, title, description, new_status);
            }
            Some(ModalState::DeleteGoal {
                goal_id,
                confirm_index,
                ..
            }) if confirm_index == CONFIRM_APPLY || confirm_index == CONFIRM_ALWAYS => {
                if confirm_index == CONFIRM_ALWAYS {
                    self.allow_delete_goal_without_confirm = true;
                }
                self.modal_submit_delete(goal_id);
            }
            Some(ModalState::DeleteGoal { .. }) => {
                // cancel
            }
            Some(ModalState::AddDeliverable {
                goal_id,
                kind,
                name,
                value,
                ..
            }) => {
                self.modal_submit_add_deliverable(goal_id, kind, name, value);
            }
            Some(ModalState::UpdateDeliverable {
                goal_id,
                deliverable_id,
                content_file,
                ..
            }) => {
                self.modal_submit_update_deliverable(goal_id, deliverable_id, content_file);
            }
            Some(ModalState::RenameDeliverable {
                goal_id,
                deliverable_id,
                name,
                ..
            }) => {
                self.modal_submit_rename_deliverable(goal_id, deliverable_id, name);
            }
            Some(ModalState::MoveDeliverable {
                goal_id,
                deliverable_id,
                targets,
                selected_index,
                ..
            }) => {
                let parent = targets
                    .get(selected_index)
                    .and_then(|(parent_id, _)| parent_id.clone());
                self.modal_submit_move_deliverable(goal_id, deliverable_id, parent);
            }
            Some(ModalState::DeleteDeliverable {
                goal_id,
                deliverable_id,
                confirm_index,
                ..
            }) if confirm_index == CONFIRM_APPLY || confirm_index == CONFIRM_ALWAYS => {
                if confirm_index == CONFIRM_ALWAYS {
                    self.allow_delete_deliverable_without_confirm = true;
                }
                self.modal_submit_delete_deliverable(goal_id, deliverable_id);
            }
            Some(ModalState::DeleteDeliverable { .. }) => {}
            Some(ModalState::AddComment { goal_id, body, .. }) => {
                self.modal_submit_add_comment(goal_id, None, body);
            }
            Some(ModalState::ReplyComment {
                goal_id,
                parent_comment_id,
                body,
                ..
            }) => {
                self.modal_submit_add_comment(goal_id, Some(parent_comment_id), body);
            }
            Some(ModalState::EditComment {
                goal_id,
                comment_id,
                body,
                ..
            }) => {
                self.modal_submit_edit_comment(goal_id, comment_id, body);
            }
            Some(ModalState::DeleteComment {
                goal_id,
                comment_id,
                confirm_index,
                ..
            }) if confirm_index == CONFIRM_APPLY || confirm_index == CONFIRM_ALWAYS => {
                if confirm_index == CONFIRM_ALWAYS {
                    self.allow_delete_comment_without_confirm = true;
                }
                self.modal_submit_delete_comment(goal_id, comment_id);
            }
            Some(ModalState::DeleteComment { .. }) => {}
            Some(ModalState::ReactComment {
                goal_id,
                comment_id,
                emojis,
                selected_index,
            }) => {
                if let Some(emoji) = emojis.get(selected_index) {
                    self.modal_submit_react_comment(goal_id, comment_id, emoji);
                }
            }
            // ファイラーは Enter を handle_file_picker_input で処理するため、ここには来ない。
            Some(ModalState::FilePicker { .. }) => {}
            None => {}
        }
    }

    fn modal_submit_create(
        &mut self,
        title: String,
        description: String,
        parent_goal_id: Option<String>,
    ) {
        // Validate
        if title.trim().is_empty() {
            self.error_message = Some("Title is required".to_string());
            return;
        }

        let Some(org_id) = self.current_org_id().map(|s| s.to_string()) else {
            self.error_message = Some("No organization selected".to_string());
            return;
        };

        let req = CreateGoalRequest {
            organization_id: org_id,
            title,
            parent_objective_id: parent_goal_id,
            description: if description.is_empty() {
                None
            } else {
                Some(description)
            },
        };

        match self.api_call(self.client.create_goal(&req)) {
            Ok(_) => {
                self.success_message = Some("Goal created successfully".to_string());
                self.load_goal_tree();
                self.load_todays_goals();
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to create goal: {e}"));
            }
        }
    }

    fn modal_submit_edit(
        &mut self,
        goal_id: String,
        title: String,
        description: String,
        new_display_status: GoalDisplayStatus,
    ) {
        // Validate
        if title.trim().is_empty() {
            self.error_message = Some("Title is required".to_string());
            return;
        }

        let Some(_org_id) = self.current_org_id().map(|s| s.to_string()) else {
            self.error_message = Some("No organization selected".to_string());
            return;
        };

        // Determine completed_at and status based on new display status
        let (api_status, completed_at) = match new_display_status {
            GoalDisplayStatus::Completed => {
                // Mark as completed: set completed_at to current time
                (GoalStatus::None, Some(Some(Utc::now().to_rfc3339())))
            }
            _ => {
                // Not completed: uncomplete if needed, set appropriate status
                (new_display_status.to_goal_status(), Some(None))
            }
        };

        let req = UpdateGoalRequest {
            title: Some(title),
            description: Some(description),
            status: Some(api_status),
            completed_at,
            body: None,
            due_date: None,
        };

        match self.api_call(self.client.update_goal(&goal_id, &req)) {
            Ok(_) => {
                self.success_message = Some("Goal updated successfully".to_string());
                self.load_goal_tree();
                self.load_todays_goals();
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to update goal: {e}"));
            }
        }
    }

    fn modal_submit_delete(&mut self, goal_id: String) {
        match self.api_call(self.client.delete_goal(&goal_id)) {
            Ok(_) => {
                self.success_message = Some("Goal deleted successfully".to_string());
                self.load_goal_tree();
                self.load_todays_goals();
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to delete goal: {e}"));
            }
        }
    }

    /// カーソル上のゴールの完了状態を切り替える。
    /// 完了は completed_at に現在時刻、再開は completed_at をクリアする
    /// （title/description は送らず最小更新）。
    fn do_set_goal_completed(&mut self, completed: bool) {
        let Some((goal_id, _title, _)) = self.selected_goal_row_context() else {
            self.error_message = Some("Please select a goal to complete".to_string());
            return;
        };

        let req = if completed {
            UpdateGoalRequest {
                status: Some(GoalStatus::None),
                completed_at: Some(Some(Utc::now().to_rfc3339())),
                title: None,
                description: None,
                body: None,
                due_date: None,
            }
        } else {
            UpdateGoalRequest {
                status: Some(GoalStatus::InProgress),
                completed_at: Some(None),
                title: None,
                description: None,
                body: None,
                due_date: None,
            }
        };

        match self.api_call(self.client.update_goal(&goal_id, &req)) {
            Ok(_) => {
                self.success_message = Some(
                    if completed {
                        "Goal completed"
                    } else {
                        "Goal reopened"
                    }
                    .to_string(),
                );
                self.load_goal_tree();
                // Execution は表示済みのときだけ再取得（遅延ロードを維持）。
                if self.todays_loaded {
                    self.load_todays_goals();
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to update goal: {e}"));
            }
        }
    }

    fn run_action_menu_item(&mut self, item: ActionMenuItem) {
        match item {
            ActionMenuItem::WorkWithCodex => self.start_codex(),
            ActionMenuItem::WorkWithClaude => self.start_agent(AgentKind::ClaudeCode),
            ActionMenuItem::AddDeliverable => self.start_add_deliverable_modal(),
            ActionMenuItem::AddComment => self.start_add_comment_modal(),
            ActionMenuItem::CompleteGoal => self.do_set_goal_completed(true),
            ActionMenuItem::ReopenGoal => self.do_set_goal_completed(false),
            ActionMenuItem::EditGoal => self.start_edit_goal_modal(),
            ActionMenuItem::DeleteGoal => self.start_delete_goal_modal(),
            ActionMenuItem::UpdateDeliverable => self.start_update_deliverable_modal(),
            ActionMenuItem::RenameDeliverable => self.start_rename_deliverable_modal(),
            ActionMenuItem::MoveDeliverable => self.start_move_deliverable_modal(),
            ActionMenuItem::DeleteDeliverable => self.start_delete_deliverable_modal(),
            ActionMenuItem::ReplyComment => self.start_reply_comment_modal(),
            ActionMenuItem::ResolveComment => self.do_set_comment_resolved(true),
            ActionMenuItem::UnresolveComment => self.do_set_comment_resolved(false),
            ActionMenuItem::EditComment => self.start_edit_comment_modal(),
            ActionMenuItem::DeleteComment => self.start_delete_comment_modal(),
            ActionMenuItem::ReactComment => self.start_react_comment_modal(),
        }
    }

    fn modal_submit_add_deliverable(
        &mut self,
        goal_id: String,
        kind: DeliverableKind,
        name: String,
        value: String,
    ) {
        let display_name = name.trim();
        let value = value.trim();

        let result = match kind {
            DeliverableKind::File => {
                if value.is_empty() {
                    self.error_message = Some("File path is required".to_string());
                    return;
                }
                let path = PathBuf::from(value);
                let display = if display_name.is_empty() {
                    None
                } else {
                    Some(display_name)
                };
                self.api_call(
                    self.client
                        .create_file_deliverable_from_path(&goal_id, &path, display),
                )
                .map(|resp| resp.data.id)
            }
            DeliverableKind::Document => {
                if value.is_empty() {
                    self.error_message = Some("Content file path is required".to_string());
                    return;
                }
                let path = PathBuf::from(value);
                let content = match std::fs::read_to_string(&path) {
                    Ok(content) => content,
                    Err(e) => {
                        self.error_message = Some(format!(
                            "Failed to read content file {}: {e}",
                            path.display()
                        ));
                        return;
                    }
                };
                let display = if display_name.is_empty() {
                    match path.file_name().and_then(|s| s.to_str()) {
                        Some(file_name) => file_name.to_string(),
                        None => {
                            self.error_message = Some("Name is required".to_string());
                            return;
                        }
                    }
                } else {
                    display_name.to_string()
                };
                self.api_call(
                    self.client
                        .create_document_deliverable(&goal_id, &display, &content),
                )
                .map(|resp| resp.data.id)
            }
            DeliverableKind::Link => {
                if display_name.is_empty() || value.is_empty() {
                    self.error_message = Some("Name and URL are required".to_string());
                    return;
                }
                self.api_call(
                    self.client
                        .create_link_deliverable(&goal_id, value, display_name),
                )
                .map(|resp| resp.data.id)
            }
            DeliverableKind::Folder => {
                if display_name.is_empty() {
                    self.error_message = Some("Name is required".to_string());
                    return;
                }
                self.api_call(
                    self.client
                        .create_folder_deliverable(&goal_id, display_name),
                )
                .map(|resp| resp.data.id)
            }
        };

        match result {
            Ok(id) => {
                self.success_message = Some(format!("Deliverable added: {id}"));
                self.reload_deliverables_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to add deliverable: {e}"));
            }
        }
    }

    fn modal_submit_update_deliverable(
        &mut self,
        goal_id: String,
        deliverable_id: String,
        content_file: String,
    ) {
        let path = PathBuf::from(content_file.trim());
        if path.as_os_str().is_empty() {
            self.error_message = Some("Content file path is required".to_string());
            return;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => {
                self.error_message = Some(format!(
                    "Failed to read content file {}: {e}",
                    path.display()
                ));
                return;
            }
        };

        match self.api_call(self.client.update_deliverable(
            &goal_id,
            &deliverable_id,
            &content,
            vec![],
        )) {
            Ok(_) => {
                self.success_message = Some("Deliverable updated".to_string());
                self.reload_deliverables_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to update deliverable: {e}"));
            }
        }
    }

    fn modal_submit_rename_deliverable(
        &mut self,
        goal_id: String,
        deliverable_id: String,
        name: String,
    ) {
        let name = name.trim();
        if name.is_empty() {
            self.error_message = Some("Name is required".to_string());
            return;
        }

        match self.api_call(
            self.client
                .rename_deliverable(&goal_id, &deliverable_id, name),
        ) {
            Ok(_) => {
                self.success_message = Some("Deliverable renamed".to_string());
                self.reload_deliverables_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to rename deliverable: {e}"));
            }
        }
    }

    fn modal_submit_move_deliverable(
        &mut self,
        goal_id: String,
        deliverable_id: String,
        parent: Option<String>,
    ) {
        match self.api_call(
            self.client
                .move_deliverable(&goal_id, &deliverable_id, parent, 0.0),
        ) {
            Ok(_) => {
                self.success_message = Some("Deliverable moved".to_string());
                self.reload_deliverables_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to move deliverable: {e}"));
            }
        }
    }

    fn modal_submit_delete_deliverable(&mut self, goal_id: String, deliverable_id: String) {
        match self.api_call(self.client.delete_deliverable(&goal_id, &deliverable_id)) {
            Ok(_) => {
                self.success_message = Some("Deliverable deleted".to_string());
                self.reload_deliverables_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to delete deliverable: {e}"));
            }
        }
    }

    fn modal_next_status(&mut self) {
        if let Some(ModalState::EditGoal {
            selected_status_index,
            allowed_statuses,
            ..
        }) = &mut self.modal_state
            && !allowed_statuses.is_empty()
        {
            *selected_status_index = (*selected_status_index + 1) % allowed_statuses.len();
        }
    }

    fn modal_prev_status(&mut self) {
        if let Some(ModalState::EditGoal {
            selected_status_index,
            allowed_statuses,
            ..
        }) = &mut self.modal_state
            && !allowed_statuses.is_empty()
        {
            *selected_status_index = if *selected_status_index == 0 {
                allowed_statuses.len() - 1
            } else {
                *selected_status_index - 1
            };
        }
    }

    fn modal_next_deliverable_kind(&mut self) {
        if let Some(ModalState::AddDeliverable { kind, .. }) = &mut self.modal_state {
            *kind = kind.next();
        }
    }

    fn modal_prev_deliverable_kind(&mut self) {
        if let Some(ModalState::AddDeliverable { kind, .. }) = &mut self.modal_state {
            *kind = kind.prev();
        }
    }

    fn modal_menu_next(&mut self) {
        if let Some(ModalState::ActionMenu {
            selected_index,
            items,
            ..
        }) = &mut self.modal_state
            && !items.is_empty()
        {
            *selected_index = (*selected_index + 1) % items.len();
        }
    }

    fn modal_menu_prev(&mut self) {
        if let Some(ModalState::ActionMenu {
            selected_index,
            items,
            ..
        }) = &mut self.modal_state
            && !items.is_empty()
        {
            *selected_index = if *selected_index == 0 {
                items.len() - 1
            } else {
                *selected_index - 1
            };
        }
    }

    fn modal_move_target_next(&mut self) {
        if let Some(ModalState::MoveDeliverable {
            selected_index,
            targets,
            ..
        }) = &mut self.modal_state
            && !targets.is_empty()
        {
            *selected_index = (*selected_index + 1) % targets.len();
        }
    }

    fn modal_move_target_prev(&mut self) {
        if let Some(ModalState::MoveDeliverable {
            selected_index,
            targets,
            ..
        }) = &mut self.modal_state
            && !targets.is_empty()
        {
            *selected_index = if *selected_index == 0 {
                targets.len() - 1
            } else {
                *selected_index - 1
            };
        }
    }

    fn modal_react_next(&mut self) {
        if let Some(ModalState::ReactComment {
            selected_index,
            emojis,
            ..
        }) = &mut self.modal_state
            && !emojis.is_empty()
        {
            *selected_index = (*selected_index + 1) % emojis.len();
        }
    }

    fn modal_react_prev(&mut self) {
        if let Some(ModalState::ReactComment {
            selected_index,
            emojis,
            ..
        }) = &mut self.modal_state
            && !emojis.is_empty()
        {
            *selected_index = if *selected_index == 0 {
                emojis.len() - 1
            } else {
                *selected_index - 1
            };
        }
    }
}

#[cfg(test)]
mod path_tests {
    use super::super::file_picker::{
        complete_path, expand_tilde, longest_common_prefix, read_dir_entries,
    };
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("addness_pathtest_{}_{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn lcp_basic() {
        assert_eq!(
            longest_common_prefix(["foobar", "foobaz", "fooqux"].into_iter()),
            "foo"
        );
        assert_eq!(longest_common_prefix(["abc"].into_iter()), "abc");
        assert_eq!(longest_common_prefix(["a", "b"].into_iter()), "");
        assert_eq!(longest_common_prefix(std::iter::empty::<&str>()), "");
    }

    #[test]
    fn expand_tilde_passthrough() {
        assert_eq!(expand_tilde("/abs/path"), "/abs/path");
        assert_eq!(expand_tilde("rel/path"), "rel/path");
        assert_eq!(expand_tilde("~notapath"), "~notapath");
    }

    #[test]
    fn complete_unique_to_full_name() {
        let d = temp_dir("unique");
        fs::write(d.join("alpha.txt"), "x").unwrap();
        fs::write(d.join("beta.txt"), "x").unwrap();
        let out = complete_path(&format!("{}/al", d.display())).unwrap();
        assert!(out.ends_with("/alpha.txt"), "got {out}");
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn complete_common_prefix_of_multiple() {
        let d = temp_dir("multi");
        fs::write(d.join("report_a.md"), "x").unwrap();
        fs::write(d.join("report_b.md"), "x").unwrap();
        let out = complete_path(&format!("{}/rep", d.display())).unwrap();
        assert!(out.ends_with("/report_"), "got {out}");
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn complete_single_dir_appends_slash() {
        let d = temp_dir("dir");
        fs::create_dir(d.join("subdir")).unwrap();
        let out = complete_path(&format!("{}/sub", d.display())).unwrap();
        assert!(out.ends_with("/subdir/"), "got {out}");
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn complete_no_match_is_none() {
        let d = temp_dir("nomatch");
        fs::write(d.join("alpha.txt"), "x").unwrap();
        assert!(complete_path(&format!("{}/zzz", d.display())).is_none());
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn entries_dirs_first_hidden_excluded() {
        let d = temp_dir("entries");
        fs::create_dir(d.join("zdir")).unwrap();
        fs::write(d.join("afile.txt"), "x").unwrap();
        fs::write(d.join(".hidden"), "x").unwrap();
        let entries = read_dir_entries(&d);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["zdir", "afile.txt"]);
        assert!(entries[0].is_dir);
        fs::remove_dir_all(&d).ok();
    }
}

#[cfg(test)]
mod picker_render_tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn render_text(app: &mut App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| super::super::ui::draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    fn app_with_picker(entries: Vec<FileEntry>, selected: usize) -> (tokio::runtime::Runtime, App) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = ApiClient::new("t", "http://localhost").unwrap();
        let mut app = App::new(client, rt.handle().clone());
        app.modal_state = Some(ModalState::FilePicker {
            dir: std::path::PathBuf::from("/tmp"),
            entries,
            selected_index: selected,
            ret: FilePickerReturn::UpdateDeliverable {
                goal_id: "g".into(),
                deliverable_id: "d".into(),
                deliverable_name: "n".into(),
                content_file: String::new(),
            },
        });
        (rt, app)
    }

    #[test]
    fn picker_selected_visible_on_short_terminal() {
        let entries: Vec<FileEntry> = (0..30)
            .map(|i| FileEntry {
                name: format!("file_{i:02}"),
                is_dir: false,
            })
            .collect();
        // 選択を末尾付近に。低い端末(高さ10)でも選択行が画面内に描画されること。
        // （scroll_offset を固定値で計算していた頃のリグレッション防止）
        let (_rt, mut app) = app_with_picker(entries, 27);
        let text = render_text(&mut app, 80, 10);
        assert!(
            text.contains("file_27"),
            "selected entry must be visible on a short terminal:\n{text}"
        );
    }

    #[test]
    fn picker_top_visible_when_selected_at_start() {
        let entries: Vec<FileEntry> = (0..30)
            .map(|i| FileEntry {
                name: format!("file_{i:02}"),
                is_dir: false,
            })
            .collect();
        let (_rt, mut app) = app_with_picker(entries, 0);
        let text = render_text(&mut app, 80, 24);
        assert!(text.contains("file_00"), "got:\n{text}");
    }

    #[test]
    fn complete_path_noop_when_already_complete() {
        // 一意な完全パスは complete_path で変化しない → Tab はフィールド送りにフォールバックする
        let d = std::env::temp_dir().join(format!("addness_verify_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("only.txt"), "x").unwrap();
        let full = format!("{}/only.txt", d.display());
        assert_eq!(complete_path(&full).as_deref(), Some(full.as_str()));
        std::fs::remove_dir_all(&d).ok();
    }
}

#[cfg(test)]
mod picker_interaction_tests {
    //! 実際のキーイベント処理経路 (`handle_modal_input`) に合成 KeyEvent を流し、
    //! Tab補完・Ctrl+Fでのファイラー起動・選択の書き戻し・フィールド送りを検証する。
    use super::*;
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("addness_itx_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn app_add_deliverable(
        value: &str,
        field: DeliverableFormField,
        kind: DeliverableKind,
    ) -> (tokio::runtime::Runtime, App) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = ApiClient::new("t", "http://localhost").unwrap();
        let mut app = App::new(client, rt.handle().clone());
        app.modal_state = Some(ModalState::AddDeliverable {
            goal_id: "g".into(),
            goal_title: "t".into(),
            kind,
            name: "n".into(),
            value: value.into(),
            current_field: field,
        });
        (rt, app)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn value_of(app: &App) -> String {
        match &app.modal_state {
            Some(ModalState::AddDeliverable { value, .. }) => value.clone(),
            _ => panic!("expected AddDeliverable modal"),
        }
    }

    #[test]
    fn tab_completes_path_value() {
        let d = tmp("complete");
        std::fs::write(d.join("alpha.txt"), "x").unwrap();
        let (_rt, mut app) = app_add_deliverable(
            &format!("{}/al", d.display()),
            DeliverableFormField::Value,
            DeliverableKind::Document,
        );
        app.handle_modal_input(key(KeyCode::Tab));
        assert!(
            value_of(&app).ends_with("/alpha.txt"),
            "got {}",
            value_of(&app)
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn ctrl_f_opens_picker_and_enter_writes_back() {
        let d = tmp("pick");
        std::fs::write(d.join("alpha.txt"), "x").unwrap();
        let (_rt, mut app) = app_add_deliverable(
            &format!("{}/", d.display()),
            DeliverableFormField::Value,
            DeliverableKind::Document,
        );
        // Ctrl+F でファイラーが開く
        app.handle_modal_input(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        assert!(
            matches!(app.modal_state, Some(ModalState::FilePicker { .. })),
            "Ctrl+F should open the file picker"
        );
        // Enter で選択中のファイルを書き戻して元モーダルへ戻る
        app.handle_modal_input(key(KeyCode::Enter));
        assert!(
            value_of(&app).ends_with("/alpha.txt"),
            "got {}",
            value_of(&app)
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn tab_advances_field_when_no_completion_progress() {
        let d = tmp("advance");
        std::fs::write(d.join("only.txt"), "x").unwrap();
        // 既に一意な完全パス → 補完で前進しない → Tab はフィールド送り(Value→Kind)
        let (_rt, mut app) = app_add_deliverable(
            &format!("{}/only.txt", d.display()),
            DeliverableFormField::Value,
            DeliverableKind::Document,
        );
        app.handle_modal_input(key(KeyCode::Tab));
        match &app.modal_state {
            Some(ModalState::AddDeliverable { current_field, .. }) => {
                assert_eq!(*current_field, DeliverableFormField::Kind);
            }
            _ => panic!("expected AddDeliverable modal"),
        }
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn plain_f_types_into_value() {
        // Ctrl修飾なしの 'f' は通常文字として value に入る（ファイラーは開かない）
        let (_rt, mut app) =
            app_add_deliverable("", DeliverableFormField::Value, DeliverableKind::Document);
        app.handle_modal_input(key(KeyCode::Char('f')));
        assert_eq!(value_of(&app), "f");
        assert!(matches!(
            app.modal_state,
            Some(ModalState::AddDeliverable { .. })
        ));
    }
}

#[cfg(test)]
mod action_menu_tests {
    use super::*;

    #[test]
    fn work_with_claude_item_has_label() {
        assert_eq!(ActionMenuItem::WorkWithClaude.label(), "claude codeで作業");
        assert_eq!(ActionMenuItem::WorkWithCodex.label(), "codexで作業");
    }
}

#[cfg(test)]
mod codex_help_key_tests {
    use super::*;

    #[test]
    fn ctrl_help_key_accepts_ctrl_q() {
        assert!(App::is_ctrl_help_key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL,
        )));
        // Ctrl+Shift+Q（大文字で届くケース）も許容する。
        assert!(App::is_ctrl_help_key(KeyEvent::new(
            KeyCode::Char('Q'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )));
    }

    #[test]
    fn ctrl_help_key_does_not_steal_plain_q_or_other_ctrl_keys() {
        // 素の q はアプリ終了などに使うため横取りしない。
        assert!(!App::is_ctrl_help_key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        )));
        // 旧割り当て（Ctrl+? / Ctrl+/）はもうヘルプを開かない。
        assert!(!App::is_ctrl_help_key(KeyEvent::new(
            KeyCode::Char('/'),
            KeyModifiers::CONTROL,
        )));
        assert!(!App::is_ctrl_help_key(KeyEvent::new(
            KeyCode::Char('f'),
            KeyModifiers::CONTROL,
        )));
    }

    fn app_for_help_scroll() -> (tokio::runtime::Runtime, App) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = ApiClient::new("t", "http://localhost").unwrap();
        let mut app = App::new(client, rt.handle().clone());
        app.show_help = true;
        app.help_scroll = 5;
        (rt, app)
    }

    #[test]
    fn help_scroll_moves_with_arrow_and_page_keys_while_open() {
        let (_rt, mut app) = app_for_help_scroll();

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))
            .unwrap();
        assert_eq!(app.help_scroll, 6);
        assert!(app.show_help);

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)))
            .unwrap();
        assert_eq!(app.help_scroll, 5);

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::PageDown,
            KeyModifiers::NONE,
        )))
        .unwrap();
        assert_eq!(app.help_scroll, 15);

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)))
            .unwrap();
        assert_eq!(app.help_scroll, 0);
    }

    #[test]
    fn help_scroll_does_not_underflow_at_top() {
        let (_rt, mut app) = app_for_help_scroll();
        app.help_scroll = 0;

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)))
            .unwrap();
        assert_eq!(app.help_scroll, 0);
    }

    #[test]
    fn help_close_keys_still_close_overlay_and_scroll_resets_next_open() {
        let (_rt, mut app) = app_for_help_scroll();

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))
            .unwrap();
        assert!(!app.show_help);

        // 素の '?' で再度開くとスクロールは先頭へ戻る。
        app.handle_normal(KeyCode::Char('?'));
        assert!(app.show_help);
        assert_eq!(app.help_scroll, 0);
    }
}

#[cfg(test)]
mod codex_turn_key_tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn modified_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn app_with_codex_turns(scrollback: usize) -> (tokio::runtime::Runtime, App) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = ApiClient::new("t", "http://localhost").unwrap();
        let mut app = App::new(client, rt.handle().clone());
        let mut pane = CodexPane::test_with_output(6, 80, 0, "");
        pane.finished = false;
        pane.test_add_completed_turn("first response");
        pane.test_add_completed_turn("second response");
        pane.scrollback = scrollback;
        app.active_pane = ActivePane::Codex;
        app.codex = Some(pane);
        (rt, app)
    }

    fn app_with_codex_history() -> (tokio::runtime::Runtime, App) {
        app_with_codex_turns(1)
    }

    fn app_with_codex_live_input() -> (tokio::runtime::Runtime, App) {
        app_with_codex_turns(0)
    }

    #[test]
    fn history_enter_toggles_visible_turn_without_submitting_input() {
        let (_rt, mut app) = app_with_codex_history();

        app.handle_codex_key(key(KeyCode::Enter));

        let pane = app.codex.as_ref().unwrap();
        assert_eq!(pane.input_line(), "");
        assert_eq!(pane.collapsed_turn_count(), 1);
    }

    #[test]
    fn history_plain_digit_returns_to_live_and_types_without_turn_shortcut() {
        let (_rt, mut app) = app_with_codex_history();

        app.handle_codex_key(key(KeyCode::Char('1')));

        let pane = app.codex.as_ref().unwrap();
        assert_eq!(pane.input_line(), "1");
        assert_eq!(pane.collapsed_turn_count(), 0);
    }

    #[test]
    fn live_empty_enter_toggles_latest_turn_without_submitting_input() {
        let (_rt, mut app) = app_with_codex_live_input();

        app.handle_codex_key(key(KeyCode::Enter));

        let pane = app.codex.as_ref().unwrap();
        assert_eq!(pane.input_line(), "");
        assert_eq!(pane.collapsed_turn_count(), 1);
    }

    #[test]
    fn live_empty_space_toggles_latest_turn_without_typing_space() {
        let (_rt, mut app) = app_with_codex_live_input();

        app.handle_codex_key(key(KeyCode::Char(' ')));

        let pane = app.codex.as_ref().unwrap();
        assert_eq!(pane.input_line(), "");
        assert_eq!(pane.collapsed_turn_count(), 1);
    }

    #[test]
    fn live_ctrl_o_toggles_latest_turn_without_typing() {
        let (_rt, mut app) = app_with_codex_live_input();

        app.handle_codex_key(modified_key(KeyCode::Char('o'), KeyModifiers::CONTROL));

        let pane = app.codex.as_ref().unwrap();
        assert_eq!(pane.input_line(), "");
        assert_eq!(pane.collapsed_turn_count(), 1);
    }

    #[test]
    fn f7_turn_picker_opens_and_enter_expands_selected_turn() {
        let (_rt, mut app) = app_with_codex_live_input();
        {
            let pane = app.codex.as_mut().unwrap();
            pane.toggle_old_turns_collapsed();
        }

        app.handle_codex_key(key(KeyCode::F(7)));
        {
            let pane = app.codex.as_ref().unwrap();
            assert!(pane.turn_picker_open());
            assert_eq!(pane.turn_picker_selected_turn(), Some(1));
            assert_eq!(pane.collapsed_turn_count(), 2);
        }

        app.handle_codex_key(key(KeyCode::Enter));

        let pane = app.codex.as_ref().unwrap();
        assert!(pane.turn_picker_open());
        assert_eq!(pane.collapsed_turn_count(), 1);
        assert!(!pane.turn_picker_items()[0].collapsed);
    }

    #[test]
    fn live_plain_digit_still_types_into_codex_prompt() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = ApiClient::new("t", "http://localhost").unwrap();
        let mut app = App::new(client, rt.handle().clone());
        let mut pane = CodexPane::test_with_output(6, 80, 0, "");
        pane.finished = false;
        app.active_pane = ActivePane::Codex;
        app.codex = Some(pane);

        app.handle_codex_key(key(KeyCode::Char('1')));

        let pane = app.codex.as_ref().unwrap();
        assert_eq!(pane.input_line(), "1");
        assert_eq!(pane.collapsed_turn_count(), 0);
    }

    #[test]
    fn question_mark_opens_help_when_input_empty() {
        let (_rt, mut app) = app_with_codex_live_input();

        app.handle_codex_key(key(KeyCode::Char('?')));

        assert!(app.show_help);
        assert_eq!(app.help_scroll, 0);
        assert_eq!(app.codex.as_ref().unwrap().input_line(), "");
    }

    #[test]
    fn question_mark_types_into_prompt_when_input_nonempty() {
        let (_rt, mut app) = app_with_codex_live_input();

        app.handle_codex_key(key(KeyCode::Char('a')));
        app.handle_codex_key(key(KeyCode::Char('?')));

        assert!(!app.show_help);
        assert_eq!(app.codex.as_ref().unwrap().input_line(), "a?");
    }

    #[test]
    fn question_mark_ignored_during_scrollback() {
        let (_rt, mut app) = app_with_codex_history();

        app.handle_codex_key(key(KeyCode::Char('?')));

        // スクロール中は `?` でヘルプを開かず、従来どおりの経路に委ねる。
        assert!(!app.show_help);
    }
}

#[cfg(test)]
mod status_message_tests {
    use super::*;

    fn app() -> (tokio::runtime::Runtime, App) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = ApiClient::new("t", "http://localhost").unwrap();
        let app = App::new(client, rt.handle().clone());
        (rt, app)
    }

    #[test]
    fn new_message_arms_deadline() {
        let (_rt, mut app) = app();
        assert!(app.status_deadline.is_none());

        app.success_message = Some("done".to_string());
        app.refresh_status_deadline();

        assert!(app.status_deadline.is_some());
        // 期限内なので消えない。
        assert!(!app.expire_status_messages());
        assert_eq!(app.success_message.as_deref(), Some("done"));
    }

    #[test]
    fn expired_message_is_cleared() {
        let (_rt, mut app) = app();
        app.error_message = Some("boom".to_string());
        app.refresh_status_deadline();

        // 期限を過去にして期限切れを再現する。
        app.status_deadline = Some(Instant::now() - Duration::from_millis(1));

        assert!(app.expire_status_messages());
        assert!(app.error_message.is_none());
        assert!(app.status_deadline.is_none());
    }

    #[test]
    fn newer_message_rearms_deadline() {
        let (_rt, mut app) = app();
        app.success_message = Some("first".to_string());
        app.refresh_status_deadline();
        // 期限切れ寸前に設定。
        app.status_deadline = Some(Instant::now() - Duration::from_millis(1));

        // 新しいメッセージが上書きされたら期限を張り直す。
        app.success_message = Some("second".to_string());
        app.refresh_status_deadline();

        assert!(app.status_deadline.is_some());
        assert!(!app.expire_status_messages());
        assert_eq!(app.success_message.as_deref(), Some("second"));
    }
}

#[cfg(test)]
mod codex_mouse_tests {
    use super::*;

    #[test]
    fn point_in_inner_area_converts_borderless_terminal_coordinates() {
        let area = Rect {
            x: 10,
            y: 5,
            width: 20,
            height: 8,
        };

        assert_eq!(App::point_in_inner_area(area, 11, 6), Some((0, 0)));
        assert_eq!(App::point_in_inner_area(area, 28, 11), Some((17, 5)));
    }

    #[test]
    fn point_in_inner_area_excludes_borders_and_outside() {
        let area = Rect {
            x: 10,
            y: 5,
            width: 20,
            height: 8,
        };

        assert_eq!(App::point_in_inner_area(area, 10, 6), None);
        assert_eq!(App::point_in_inner_area(area, 11, 5), None);
        assert_eq!(App::point_in_inner_area(area, 29, 6), None);
        assert_eq!(App::point_in_inner_area(area, 11, 12), None);
    }

    #[test]
    fn point_in_inner_area_ignores_tiny_rects() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 1,
        };

        assert_eq!(App::point_in_inner_area(area, 1, 1), None);
    }

    #[test]
    fn point_in_area_includes_frame_borders() {
        let area = Rect {
            x: 10,
            y: 5,
            width: 20,
            height: 8,
        };

        assert!(App::point_in_area(Some(area), 10, 5));
        assert!(App::point_in_area(Some(area), 29, 12));
        assert!(!App::point_in_area(Some(area), 30, 12));
        assert!(!App::point_in_area(Some(area), 29, 13));
        assert!(!App::point_in_area(None, 10, 5));
    }

    #[test]
    fn mouse_scroll_delta_maps_trackpad_wheel_to_history_direction() {
        assert_eq!(
            App::mouse_scroll_delta(MouseEventKind::ScrollUp),
            Some(CODEX_WHEEL_LINES)
        );
        assert_eq!(
            App::mouse_scroll_delta(MouseEventKind::ScrollDown),
            Some(-CODEX_WHEEL_LINES)
        );
        assert_eq!(
            App::mouse_scroll_delta(MouseEventKind::ScrollLeft),
            Some(CODEX_WHEEL_LINES)
        );
        assert_eq!(
            App::mouse_scroll_delta(MouseEventKind::ScrollRight),
            Some(-CODEX_WHEEL_LINES)
        );
        assert_eq!(App::mouse_scroll_delta(MouseEventKind::Moved), None);
    }

    #[test]
    fn mouse_capture_enables_sgr_without_motion_reporting() {
        let enable = String::from_utf8(XTERM_MOUSE_CAPTURE_ON.to_vec()).unwrap();

        assert!(enable.contains("\x1b[?1000h"), "normal tracking missing");
        assert!(enable.contains("\x1b[?1006h"), "SGR mouse mode missing");
        assert!(
            !enable.contains("\x1b[?1002h"),
            "button-event tracking should stay disabled"
        );
        assert!(
            !enable.contains("\x1b[?1003h"),
            "any-event tracking should stay disabled"
        );

        let disable = String::from_utf8(XTERM_MOUSE_CAPTURE_OFF.to_vec()).unwrap();

        assert!(
            disable.contains("\x1b[?1006l"),
            "SGR mouse mode cleanup missing"
        );
        assert!(
            disable.contains("\x1b[?1000l"),
            "normal tracking cleanup missing"
        );
    }

    #[test]
    fn scroll_index_saturates_at_zero() {
        let mut offset = 1;
        App::scroll_index(&mut offset, 3);
        assert_eq!(offset, 4);

        App::scroll_index(&mut offset, -10);
        assert_eq!(offset, 0);
    }

    #[test]
    fn codex_terminal_scroll_route_prefers_scrollback() {
        assert_eq!(
            App::codex_terminal_scroll_route(0, 3, true),
            CodexTerminalScrollRoute::Scrollback
        );
        assert_eq!(
            App::codex_terminal_scroll_route(0, 0, true),
            CodexTerminalScrollRoute::None
        );
        assert_eq!(
            App::codex_terminal_scroll_route(0, 0, false),
            CodexTerminalScrollRoute::None
        );
    }

    #[test]
    fn handle_codex_mouse_scrolls_real_codex_pane_scrollback() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = ApiClient::new("t", "http://localhost").unwrap();
        let mut app = App::new(client, rt.handle().clone());
        let mut output = String::new();
        for row in 1..=100 {
            output.push_str(&format!("row {row:03}\n"));
        }
        app.codex = Some(CodexPane::test_with_output(100, 20, 0, &output));
        app.codex.as_mut().unwrap().resize(4, 20);
        app.active_pane = ActivePane::Codex;
        app.codex_terminal_area = Some(Rect {
            x: 10,
            y: 5,
            width: 22,
            height: 6,
        });

        app.handle_codex_mouse_scroll(
            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 20,
                row: 8,
                modifiers: KeyModifiers::NONE,
            },
            CODEX_WHEEL_LINES,
            1,
        );

        let pane = app.codex.as_ref().unwrap();
        assert_eq!(pane.scrollback, CODEX_WHEEL_LINES as usize);
        assert_eq!(
            app.codex_last_scroll_input.as_deref(),
            Some("mouse ScrollUp -> codex 0->6")
        );
    }

    #[test]
    fn handle_codex_mouse_scroll_applies_coalesced_trackpad_delta() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = ApiClient::new("t", "http://localhost").unwrap();
        let mut app = App::new(client, rt.handle().clone());
        let mut output = String::new();
        for row in 1..=100 {
            output.push_str(&format!("row {row:03}\n"));
        }
        app.codex = Some(CodexPane::test_with_output(100, 20, 0, &output));
        app.codex.as_mut().unwrap().resize(4, 20);
        app.active_pane = ActivePane::Codex;
        app.codex_terminal_area = Some(Rect {
            x: 10,
            y: 5,
            width: 22,
            height: 6,
        });

        app.handle_codex_mouse_scroll(
            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 20,
                row: 8,
                modifiers: KeyModifiers::NONE,
            },
            CODEX_WHEEL_LINES * 2,
            2,
        );

        let pane = app.codex.as_ref().unwrap();
        assert_eq!(pane.scrollback, (CODEX_WHEEL_LINES * 2) as usize);
        assert_eq!(
            app.codex_last_scroll_input.as_deref(),
            Some("mouse ScrollUp x2 -> codex 0->12")
        );
    }

    #[test]
    fn handle_codex_mouse_scroll_routes_left_panes_by_pointer_area() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = ApiClient::new("t", "http://localhost").unwrap();
        let mut app = App::new(client, rt.handle().clone());
        app.codex = Some(CodexPane::test_with_output(20, 20, 0, ""));
        app.active_pane = ActivePane::Codex;
        app.codex_status_area = Some(Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 5,
        });
        app.codex_contract_area = Some(Rect {
            x: 0,
            y: 5,
            width: 20,
            height: 10,
        });
        app.codex_activity_area = Some(Rect {
            x: 0,
            y: 15,
            width: 20,
            height: 5,
        });

        app.handle_codex_mouse_scroll(
            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 2,
                row: 6,
                modifiers: KeyModifiers::NONE,
            },
            CODEX_WHEEL_LINES,
            1,
        );
        assert_eq!(app.codex_contract_scroll, 0);
        assert_eq!(
            app.codex_last_scroll_input.as_deref(),
            Some("mouse ScrollUp -> Addnessゴール")
        );

        app.handle_codex_mouse_scroll(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 2,
                row: 6,
                modifiers: KeyModifiers::NONE,
            },
            -CODEX_WHEEL_LINES,
            1,
        );
        assert_eq!(app.codex_contract_scroll, CODEX_WHEEL_LINES as usize);
        assert_eq!(
            app.codex_last_scroll_input.as_deref(),
            Some("mouse ScrollDown -> Addnessゴール")
        );

        app.handle_codex_mouse_scroll(
            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 2,
                row: 16,
                modifiers: KeyModifiers::NONE,
            },
            CODEX_WHEEL_LINES,
            1,
        );
        assert_eq!(app.codex_activity_scroll, CODEX_WHEEL_LINES as usize);
        assert_eq!(
            app.codex_last_scroll_input.as_deref(),
            Some("mouse ScrollUp -> Addness更新")
        );

        app.handle_codex_mouse_scroll(
            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 2,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            CODEX_WHEEL_LINES,
            1,
        );
        assert_eq!(app.codex_contract_scroll, 0);
    }

    #[test]
    fn shifted_codex_navigation_keys_are_swallowed() {
        assert!(App::is_codex_shift_navigation_key(KeyEvent::new(
            KeyCode::Up,
            KeyModifiers::SHIFT,
        )));
        assert!(App::is_codex_shift_navigation_key(KeyEvent::new(
            KeyCode::PageDown,
            KeyModifiers::SHIFT,
        )));
        assert!(!App::is_codex_shift_navigation_key(KeyEvent::new(
            KeyCode::Up,
            KeyModifiers::NONE,
        )));
        assert!(!App::is_codex_shift_navigation_key(KeyEvent::new(
            KeyCode::Char('k'),
            KeyModifiers::SHIFT,
        )));
    }
}

#[cfg(test)]
mod dod_tests {
    use super::super::codex_memory::{
        CODEX_DECISION_LOG_END, CODEX_DECISION_LOG_START, CODEX_TRACEABILITY_END,
        CODEX_TRACEABILITY_START, CODEX_WORK_MEMO_END, CODEX_WORK_MEMO_START,
        codex_trace_link_label, ensure_codex_memory_sections, upsert_codex_auto_record,
    };
    use super::{extract_json_object, is_permission_denied_error_text, parse_dod_results};

    #[test]
    fn extract_json_object_strips_surrounding_text() {
        let s = "前置き {\"a\":1} 後置き";
        assert_eq!(extract_json_object(s).as_deref(), Some("{\"a\":1}"));
    }

    #[test]
    fn extract_json_object_none_when_no_braces() {
        assert!(extract_json_object("no json here").is_none());
    }

    #[test]
    fn parse_dod_results_basic() {
        let content = r#"{"results":[{"index":0,"met":true},{"index":1,"met":false}]}"#;
        let parsed = parse_dod_results(content, 2).unwrap();
        assert_eq!(parsed, vec![(0, true), (1, false)]);
    }

    #[test]
    fn parse_dod_results_with_surrounding_text() {
        let content = "判定結果です:\n{\"results\":[{\"index\":0,\"met\":true}]}\n以上";
        let parsed = parse_dod_results(content, 3).unwrap();
        assert_eq!(parsed, vec![(0, true)]);
    }

    #[test]
    fn parse_dod_results_drops_out_of_range_index() {
        let content = r#"{"results":[{"index":5,"met":true},{"index":0,"met":true}]}"#;
        let parsed = parse_dod_results(content, 2).unwrap();
        assert_eq!(parsed, vec![(0, true)]);
    }

    #[test]
    fn parse_dod_results_rejects_malformed() {
        assert!(parse_dod_results("not json", 2).is_none());
        assert!(parse_dod_results(r#"{"results":"nope"}"#, 2).is_none());
    }

    #[test]
    fn parse_dod_results_skips_broken_items_without_dropping_all() {
        // 負の index・型違いの項目は飛ばし、正常な項目は残す。
        let content =
            r#"{"results":[{"index":-1,"met":true},{"index":1,"met":"x"},{"index":0,"met":true}]}"#;
        let parsed = parse_dod_results(content, 2).unwrap();
        assert_eq!(parsed, vec![(0, true)]);
    }

    #[test]
    fn upsert_codex_auto_record_appends_to_existing_body() {
        let body = upsert_codex_auto_record(Some("手書きメモ"), "自動記録1");

        assert!(body.contains("手書きメモ"));
        assert!(body.contains("自動記録1"));
    }

    #[test]
    fn upsert_codex_auto_record_replaces_existing_block() {
        let first = upsert_codex_auto_record(Some("手書きメモ"), "自動記録1");
        let second = upsert_codex_auto_record(Some(&first), "自動記録2");

        assert!(second.contains("手書きメモ"));
        assert!(!second.contains("自動記録1"));
        assert!(second.contains("自動記録2"));
    }

    #[test]
    fn ensure_codex_memory_sections_adds_memory_decision_and_trace_blocks_once() {
        let first = ensure_codex_memory_sections("手書きメモ".to_string());
        let second = ensure_codex_memory_sections(first.clone());

        assert!(second.contains("手書きメモ"));
        assert_eq!(second.matches("## Codex作業メモ").count(), 1);
        assert_eq!(second.matches("## Codex決定ログ").count(), 1);
        assert_eq!(second.matches("## PR/Release Traceability").count(), 1);
        assert!(second.contains("Codexの通常memoryへ混ぜない"));
    }

    #[test]
    fn codex_trace_link_label_detects_pr_and_release_links() {
        assert_eq!(
            codex_trace_link_label(
                "AddnessTech/Addness-cli#92",
                Some("https://github.com/AddnessTech/Addness-cli/pull/92")
            )
            .as_deref(),
            Some("PR: AddnessTech/Addness-cli#92")
        );
        assert_eq!(
            codex_trace_link_label(
                "Release v0.5.7",
                Some("https://github.com/AddnessTech/Addness-cli/releases/tag/v0.5.7")
            )
            .as_deref(),
            Some("Release: Release v0.5.7")
        );
        assert!(codex_trace_link_label("Design note", Some("https://example.com")).is_none());
        // ベア部分一致で誤検知していたケース（"staging" は s-tag-ing を含む）。
        assert!(
            codex_trace_link_label("staging環境デプロイ", Some("https://example.com/staging"))
                .is_none()
        );
        assert!(
            codex_trace_link_label("press release draft", Some("https://example.com/doc"))
                .is_none()
        );
    }

    #[test]
    fn permission_denied_error_text_detects_goal_write_errors() {
        assert!(is_permission_denied_error_text(
            "API error (403 Forbidden): objective.update denied"
        ));
        assert!(is_permission_denied_error_text(
            "この操作を行う権限がありません"
        ));
        assert!(!is_permission_denied_error_text("API error (500): server"));
    }

    #[test]
    fn ensure_codex_memory_sections_skips_when_only_heading_remains() {
        let seeded = ensure_codex_memory_sections(String::new());
        // codex が body を編集して不可視マーカーだけ落とした状況を模す。
        let without_markers = seeded
            .replace(CODEX_WORK_MEMO_START, "")
            .replace(CODEX_WORK_MEMO_END, "")
            .replace(CODEX_DECISION_LOG_START, "")
            .replace(CODEX_DECISION_LOG_END, "")
            .replace(CODEX_TRACEABILITY_START, "")
            .replace(CODEX_TRACEABILITY_END, "");
        let again = ensure_codex_memory_sections(without_markers);

        assert_eq!(again.matches("## Codex作業メモ").count(), 1);
        assert_eq!(again.matches("## Codex決定ログ").count(), 1);
        assert_eq!(again.matches("## PR/Release Traceability").count(), 1);
    }
}
