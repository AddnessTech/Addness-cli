use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::{
    api::{Comment, Deliverable, GoalChildItem, GoalStatus, GoalTreeItem, Owner, TodaysGoalNode},
    dbg_log,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const INITIAL_COMMENT_DISPLAY_LIMIT: usize = 20;
const COMMENT_DISPLAY_INCREMENT: usize = 20;

// ---------------------------------------------------------------------------
// CommentView – global comment visibility mode
// ---------------------------------------------------------------------------

/// ツリー全体のコメント表示モード。`C` キーで循環する。
/// いずれもキャッシュ済みコメントの描画切替のみで、追加のAPI取得は行わない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommentView {
    /// コメント行を一切表示しない（件数バッジのみ）
    Hidden,
    /// 未解決コメントのみ表示（既定）
    #[default]
    Unresolved,
    /// 解決済みを含む全コメントを表示
    All,
}

impl CommentView {
    /// Hidden → Unresolved → All → Hidden の順で循環する。
    pub fn cycle(self) -> Self {
        match self {
            CommentView::Hidden => CommentView::Unresolved,
            CommentView::Unresolved => CommentView::All,
            CommentView::All => CommentView::Hidden,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CommentView::Hidden => "hidden",
            CommentView::Unresolved => "unresolved",
            CommentView::All => "all",
        }
    }

    /// このモードでコメントを表示するか。
    fn shows_comments(self) -> bool {
        !matches!(self, CommentView::Hidden)
    }

    /// このモードで対象コメントを表示対象に含めるか。
    fn includes(self, comment: &Comment) -> bool {
        match self {
            CommentView::Hidden => false,
            CommentView::Unresolved => comment.resolved_at.is_none(),
            CommentView::All => true,
        }
    }
}

/// 指定モードで表示対象となるコメント件数を数える。
fn visible_comment_count(comments: &[Comment], view: CommentView) -> usize {
    comments.iter().filter(|c| view.includes(c)).count()
}

/// 未解決コメント件数（バッジ表示用、モード非依存）。
fn unresolved_comment_count(comments: &[Comment]) -> usize {
    comments.iter().filter(|c| c.resolved_at.is_none()).count()
}

// ---------------------------------------------------------------------------
// Timestamped wrapper
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Timestamped<T> {
    pub data: T,

    // 将来的にfetch時刻からのキャッシュTTLの実装に用いる
    #[allow(dead_code)]
    pub fetched_at: DateTime<Utc>,
}

impl<T> Timestamped<T> {
    pub fn now(data: T) -> Self {
        Self {
            data,
            fetched_at: Utc::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// GoalItemAccessor – trait for unified access to GoalTreeItem / GoalChildItem
// ---------------------------------------------------------------------------

pub trait GoalItemAccessor {
    fn id(&self) -> &str;
    fn title(&self) -> &str;
    fn description(&self) -> Option<&str>;
    fn status(&self) -> Option<&GoalStatus>;
    fn is_completed(&self) -> bool;
    fn has_children(&self) -> bool;
    fn owner(&self) -> Option<&Owner>;
}

impl GoalItemAccessor for GoalTreeItem {
    fn id(&self) -> &str {
        &self.id
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn description(&self) -> Option<&str> {
        None
    }
    fn status(&self) -> Option<&GoalStatus> {
        self.status.as_ref()
    }
    fn is_completed(&self) -> bool {
        self.is_completed
    }
    fn has_children(&self) -> bool {
        self.has_children
    }
    fn owner(&self) -> Option<&Owner> {
        self.owner.as_ref()
    }
}

impl GoalItemAccessor for GoalChildItem {
    fn id(&self) -> &str {
        &self.id
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
    fn status(&self) -> Option<&GoalStatus> {
        self.status.as_ref()
    }
    fn is_completed(&self) -> bool {
        self.is_completed
    }
    fn has_children(&self) -> bool {
        self.has_children
    }
    fn owner(&self) -> Option<&Owner> {
        self.owner.as_ref()
    }
}

// ---------------------------------------------------------------------------
// GoalNodeInner<S> – generic tree node
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct GoalNodeInner<S> {
    pub node: S,
    pub children: Option<Timestamped<Vec<GoalChildNode>>>,
    pub comments: Option<Timestamped<Vec<Comment>>>,
    pub deliverables: Option<Timestamped<Vec<Deliverable>>>,
    pub expanded: bool,
    pub comment_display_limit: usize,
}

pub type GoalRootNode = GoalNodeInner<GoalTreeItem>;
pub type GoalChildNode = GoalNodeInner<GoalChildItem>;

// ---------------------------------------------------------------------------
// GoalTree – the full tree + cursor state
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct GoalTree {
    pub roots: Vec<GoalRootNode>,
    pub cursor: usize,
    pub scroll_offset: usize,
    /// If true, this tree is complete and should not fetch additional data on expand
    pub is_required_to_fetch: bool,
    /// コメントの表示モード（ツリー全体に適用）
    pub comment_view: CommentView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeGuide {
    depth: usize,
    ancestor_has_next: Vec<bool>,
    is_last: bool,
}

impl TreeGuide {
    fn root(is_last: bool) -> Self {
        Self {
            depth: 0,
            ancestor_has_next: Vec::new(),
            is_last,
        }
    }

    fn child(&self, is_last: bool) -> Self {
        let mut ancestor_has_next = self.ancestor_has_next.clone();
        ancestor_has_next.push(!self.is_last);

        Self {
            depth: self.depth + 1,
            ancestor_has_next,
            is_last,
        }
    }

    pub fn depth(&self) -> usize {
        self.depth
    }

    pub fn prefix(&self) -> String {
        let mut prefix = String::new();
        for has_next in &self.ancestor_has_next {
            prefix.push_str(if *has_next { "│   " } else { "    " });
        }
        prefix.push_str(if self.is_last {
            "└── "
        } else {
            "├── "
        });
        prefix
    }
}

// ---------------------------------------------------------------------------
// TreeRow – one flattened row for rendering
// ---------------------------------------------------------------------------

pub enum TreeRow<'a> {
    Goal {
        goal_id: &'a str,
        title: &'a str,
        status: Option<&'a GoalStatus>,
        owner_name: Option<&'a str>,
        is_completed: bool,
        expanded: bool,
        has_children: bool,
        children_loaded: bool,
        comments_loaded: bool,
        deliverables_loaded: bool,
        /// 未解決コメント件数（コメント読込済みのゴールのみ Some）。バッジ表示に使う。
        unresolved_comments: Option<usize>,
        depth: usize,
        guide: TreeGuide,
    },
    Detail {
        status: Option<&'a GoalStatus>,
        owner_name: Option<&'a str>,
        is_completed: bool,
        description: Option<&'a str>,
        depth: usize,
        guide: TreeGuide,
    },
    CommentHeader {
        count: usize,
        depth: usize,
        guide: TreeGuide,
    },
    CommentOmitted {
        count: usize,
        depth: usize,
        guide: TreeGuide,
    },
    CommentItem {
        comment: &'a Comment,
        depth: usize,
        guide: TreeGuide,
    },
    DeliverableHeader {
        count: usize,
        depth: usize,
        guide: TreeGuide,
    },
    DeliverableItem {
        deliverable: &'a Deliverable,
        depth: usize,
        guide: TreeGuide,
    },
}

impl<'a> TreeRow<'a> {
    pub fn depth(&self) -> usize {
        match self {
            TreeRow::Goal { depth, .. }
            | TreeRow::Detail { depth, .. }
            | TreeRow::CommentHeader { depth, .. }
            | TreeRow::CommentOmitted { depth, .. }
            | TreeRow::CommentItem { depth, .. }
            | TreeRow::DeliverableHeader { depth, .. }
            | TreeRow::DeliverableItem { depth, .. } => *depth,
        }
    }

    pub fn is_goal(&self) -> bool {
        matches!(self, TreeRow::Goal { .. })
    }
}

// ---------------------------------------------------------------------------
// Tree construction
// ---------------------------------------------------------------------------

impl GoalTree {
    pub fn empty() -> Self {
        GoalTree {
            roots: vec![],
            cursor: 0,
            scroll_offset: 0,
            is_required_to_fetch: true,
            comment_view: CommentView::default(),
        }
    }

    pub fn from_tree_items(items: Vec<GoalTreeItem>) -> Self {
        use std::collections::{HashMap, HashSet};

        // The default TUI tree is the active-goal view. The API normally excludes
        // completed goals, but defensively drop leaked completed rows here so an
        // orphaned completed child is not promoted to the top level.
        let items = items
            .into_iter()
            .filter(|item| !item.is_completed)
            .collect::<Vec<_>>();

        // Build a set of all item IDs in this response
        let id_set: HashSet<String> = items.iter().map(|item| item.id.clone()).collect();

        // Group items by parent_id for building the tree
        let mut children_by_parent: HashMap<String, Vec<GoalTreeItem>> = HashMap::new();
        let mut root_items: Vec<GoalTreeItem> = Vec::new();

        for item in items {
            match &item.parent_id {
                None => {
                    // True root goal
                    root_items.push(item);
                }
                Some(parent_id) => {
                    if !id_set.contains(parent_id) {
                        // Orphan (parent not in response) - treat as root
                        root_items.push(item);
                    } else {
                        // Child item - add to parent's children
                        children_by_parent
                            .entry(parent_id.clone())
                            .or_default()
                            .push(item);
                    }
                }
            }
        }

        // Build root nodes with their immediate children (2nd level)
        let roots = root_items
            .into_iter()
            .map(|item| {
                let item_id = item.id.clone();

                // Get children for this root goal (2nd level)
                let children = children_by_parent.remove(&item_id).map(|child_items| {
                    let child_nodes: Vec<GoalChildNode> = child_items
                        .into_iter()
                        .map(|child_item| {
                            // Convert GoalTreeItem to GoalChildItem
                            let child_goal_item = GoalChildItem {
                                id: child_item.id,
                                title: child_item.title,
                                description: None, // GoalTreeItem doesn't have description
                                parent_id: child_item.parent_id,
                                status: child_item.status,
                                is_completed: child_item.is_completed,
                                has_children: child_item.has_children,
                                order_no: child_item.order_no,
                                owner: child_item.owner,
                            };

                            GoalChildNode {
                                node: child_goal_item,
                                children: None,
                                comments: None,
                                deliverables: None,
                                expanded: false,
                                comment_display_limit: INITIAL_COMMENT_DISPLAY_LIMIT,
                            }
                        })
                        .collect();

                    Timestamped::now(child_nodes)
                });

                GoalRootNode {
                    node: item,
                    children,
                    comments: None,
                    deliverables: None,
                    expanded: true, // Auto-expand root goals to show 2nd level
                    comment_display_limit: INITIAL_COMMENT_DISPLAY_LIMIT,
                }
            })
            .collect();

        GoalTree {
            roots,
            cursor: 0,
            scroll_offset: 0,
            is_required_to_fetch: true,
            comment_view: CommentView::default(),
        }
    }

    /// Create a tree from today's goal nodes (from todays-goals API).
    /// The nodes are already filtered by the server and include execution records.
    /// All nodes are pre-expanded since the complete tree structure is provided.
    pub fn from_todays_goal_nodes(nodes: Vec<TodaysGoalNode>) -> Self {
        dbg_log!(
            "=== from_todays_goal_nodes: building complete tree from {} nodes ===",
            nodes.len()
        );

        if nodes.is_empty() {
            return Self::empty();
        }

        // Build lookup maps
        let node_map: HashMap<String, TodaysGoalNode> = nodes
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect();

        // Group nodes by parent_id
        let mut children_map: HashMap<Option<String>, Vec<String>> = HashMap::new();
        for (id, node) in &node_map {
            children_map
                .entry(node.parent_id.clone())
                .or_default()
                .push(id.clone());
        }

        // Sort children by order_no
        for child_ids in children_map.values_mut() {
            child_ids.sort_by(|a, b| {
                let order_a = node_map.get(a).map(|n| n.order_no).unwrap_or(0.0);
                let order_b = node_map.get(b).map(|n| n.order_no).unwrap_or(0.0);
                order_a
                    .partial_cmp(&order_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        // Build root nodes for this response. A today's-goals payload can contain
        // a scoped slice whose real parent is outside the response, so treat those
        // orphans as roots of the visible tree instead of dropping them.
        let mut root_ids = children_map.get(&None).cloned().unwrap_or_default();
        for (id, node) in &node_map {
            if node
                .parent_id
                .as_ref()
                .is_some_and(|parent_id| !node_map.contains_key(parent_id))
            {
                root_ids.push(id.clone());
            }
        }
        root_ids.sort_by(|a, b| {
            let order_a = node_map.get(a).map(|n| n.order_no).unwrap_or(0.0);
            let order_b = node_map.get(b).map(|n| n.order_no).unwrap_or(0.0);
            order_a
                .partial_cmp(&order_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        dbg_log!("Found {} root nodes", root_ids.len());

        let roots: Vec<GoalRootNode> = root_ids
            .iter()
            .filter_map(|id| {
                let node = node_map.get(id)?;
                Some(build_goal_node_from_todays(node, &node_map, &children_map))
            })
            .collect();

        let tree = GoalTree {
            roots,
            cursor: 0,
            scroll_offset: 0,
            is_required_to_fetch: false, // This tree is completed, no need to fetch more data
            comment_view: CommentView::default(),
        };

        dbg_log!("Built complete tree with {} roots", tree.roots.len());

        tree
    }
}

// ---------------------------------------------------------------------------
// Tree operations
// ---------------------------------------------------------------------------

impl GoalTree {
    pub fn flatten(&self) -> Vec<TreeRow<'_>> {
        let mut rows = Vec::new();
        let root_count = self.roots.len();
        for (idx, root) in self.roots.iter().enumerate() {
            let guide = TreeGuide::root(idx + 1 == root_count);
            flatten_node(root, self.comment_view, guide, &mut rows);
        }
        rows
    }

    /// コメント表示モードを Hidden → Unresolved → All → … と循環させる。
    pub fn cycle_comment_view(&mut self) {
        self.comment_view = self.comment_view.cycle();
        self.clamp_cursor();
    }

    /// 行数が減ってカーソルが範囲外に残らないよう末尾にクランプする。
    /// 表示モード切替やコメント・成果物の再取得で行数が変わった後に呼ぶ。
    pub fn clamp_cursor(&mut self) {
        let len = self.flatten().len();
        if len > 0 && self.cursor >= len {
            self.cursor = len - 1;
        }
    }

    pub fn toggle_expand(&mut self) {
        let view = self.comment_view;
        let rows = self.flatten();
        if let Some(TreeRow::Goal { .. }) = rows.get(self.cursor) {
            let mut idx = 0;
            for root in &mut self.roots {
                if toggle_at(root, self.cursor, &mut idx, view) {
                    return;
                }
            }
        }
    }

    /// Insert fetched children into the Goal node at the current cursor position.
    pub fn set_children_at_cursor(&mut self, children: Vec<GoalChildItem>) {
        let view = self.comment_view;
        let target = self.cursor;
        let mut idx = 0;
        let mut children = Some(children);
        for root in &mut self.roots {
            if set_children_at(root, target, &mut idx, &mut children, view) {
                return;
            }
        }
    }

    /// Insert fetched comments into the Goal node at the current cursor position.
    pub fn set_comments_at_cursor(&mut self, comments: Vec<Comment>) {
        let view = self.comment_view;
        let target = self.cursor;
        let mut idx = 0;
        let mut comments = Some(comments);
        for root in &mut self.roots {
            if set_comments_at(root, target, &mut idx, &mut comments, view) {
                return;
            }
        }
    }

    /// Insert fetched comments into the Goal node with the given goal_id.
    pub fn set_comments_for_goal_id(&mut self, goal_id: &str, comments: Vec<Comment>) {
        let mut comments = Some(comments);
        for root in &mut self.roots {
            if set_comments_by_id(root, goal_id, &mut comments) {
                return;
            }
        }
    }

    /// Insert fetched deliverables into the Goal node at the current cursor position.
    pub fn set_deliverables_at_cursor(&mut self, deliverables: Vec<Deliverable>) {
        let view = self.comment_view;
        let target = self.cursor;
        let mut idx = 0;
        let mut deliverables = Some(deliverables);
        for root in &mut self.roots {
            if set_deliverables_at(root, target, &mut idx, &mut deliverables, view) {
                return;
            }
        }
    }

    /// Insert fetched deliverables into the Goal node with the given goal_id.
    pub fn set_deliverables_for_goal_id(&mut self, goal_id: &str, deliverables: Vec<Deliverable>) {
        let mut deliverables = Some(deliverables);
        for root in &mut self.roots {
            if set_deliverables_by_id(root, goal_id, &mut deliverables) {
                return;
            }
        }
    }

    /// Increase the comment display limit for the goal containing the cursor.
    /// Used when user expands on CommentOmitted row to show more comments.
    pub fn increase_comment_limit(&mut self) {
        let view = self.comment_view;
        let target = self.cursor;
        let mut idx = 0;
        for root in &mut self.roots {
            if increase_comment_limit_at(root, target, &mut idx, view) {
                return;
            }
        }
    }

    pub fn cursor_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn cursor_down(&mut self) {
        let len = self.flatten().len();
        if self.cursor + 1 < len {
            self.cursor += 1;
        }
    }

    /// h/← : if on an expanded Goal, collapse it; otherwise jump to parent Goal row
    pub fn collapse_or_parent(&mut self) {
        let rows = self.flatten();
        match rows.get(self.cursor) {
            Some(TreeRow::Goal { expanded: true, .. }) => {
                self.toggle_expand();
            }
            _ => {
                let current_depth = rows.get(self.cursor).map(|r| r.depth()).unwrap_or(0);
                for i in (0..self.cursor).rev() {
                    if let TreeRow::Goal { depth, .. } = &rows[i]
                        && *depth < current_depth
                    {
                        self.cursor = i;
                        return;
                    }
                }
                if rows.get(self.cursor).is_some_and(|r| r.is_goal()) {
                    self.toggle_expand();
                }
            }
        }
    }

    /// Ensure cursor is visible within the viewport
    pub fn adjust_scroll(&mut self, viewport_height: usize) {
        if viewport_height == 0 {
            return;
        }
        if self.cursor < self.scroll_offset {
            self.scroll_offset = self.cursor;
        } else if self.cursor >= self.scroll_offset + viewport_height {
            self.scroll_offset = self.cursor - viewport_height + 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Generic helpers (no duplication between Root and Child)
// ---------------------------------------------------------------------------

/// Calculate the number of rows occupied by comments when expanded.
/// Counts comments visible under the given `view` (Hidden → 0 rows).
/// Includes header, up to `limit` most recent items, and omitted row if total > limit.
fn comment_row_count(
    comments: &Option<Timestamped<Vec<Comment>>>,
    limit: usize,
    view: CommentView,
) -> usize {
    if !view.shows_comments() {
        return 0;
    }
    match comments {
        Some(ts) => {
            let visible = visible_comment_count(&ts.data, view);
            let displayed = visible.min(limit);
            let has_omitted = visible > limit;
            1 + displayed + if has_omitted { 1 } else { 0 } // header + items + omitted row
        }
        None => 0,
    }
}

fn flatten_node<'a, S: GoalItemAccessor>(
    node: &'a GoalNodeInner<S>,
    view: CommentView,
    guide: TreeGuide,
    rows: &mut Vec<TreeRow<'a>>,
) {
    let unresolved_badge = node
        .comments
        .as_ref()
        .map(|ts| unresolved_comment_count(&ts.data));

    rows.push(TreeRow::Goal {
        goal_id: node.node.id(),
        title: node.node.title(),
        status: node.node.status(),
        owner_name: node.node.owner().map(|o| o.name.as_str()),
        is_completed: node.node.is_completed(),
        expanded: node.expanded,
        has_children: node.node.has_children(),
        children_loaded: node.children.is_some(),
        comments_loaded: node.comments.is_some(),
        deliverables_loaded: node.deliverables.is_some(),
        unresolved_comments: unresolved_badge,
        depth: guide.depth(),
        guide: guide.clone(),
    });

    if !node.expanded {
        return;
    }

    // Hidden モードではコメント行を一切生成しない（件数バッジのみで存在を示す）。
    let show_comments = view.shows_comments() && node.comments.is_some();
    let has_deliverables = node.deliverables.is_some();
    let has_children = node.children.as_ref().is_some_and(|ts| !ts.data.is_empty());

    let detail_guide = guide.child(!show_comments && !has_deliverables && !has_children);
    rows.push(TreeRow::Detail {
        status: node.node.status(),
        owner_name: node.node.owner().map(|o| o.name.as_str()),
        is_completed: node.node.is_completed(),
        description: node.node.description(),
        depth: detail_guide.depth(),
        guide: detail_guide,
    });

    if let Some(ref ts) = node.comments
        && view.shows_comments()
    {
        // 表示モードに合致するコメントのみ対象にする。
        let mut visible_comments: Vec<&Comment> =
            ts.data.iter().filter(|c| view.includes(c)).collect();

        let total_visible = visible_comments.len();
        let display_count = total_visible.min(node.comment_display_limit);
        let omitted_count = total_visible.saturating_sub(node.comment_display_limit);
        let comment_guide = guide.child(!has_deliverables && !has_children);

        rows.push(TreeRow::CommentHeader {
            count: total_visible,
            depth: comment_guide.depth(),
            guide: comment_guide.clone(),
        });

        // Sort by created_at descending (newest first)
        visible_comments.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        if omitted_count > 0 {
            let omitted_guide = comment_guide.child(display_count == 0);
            rows.push(TreeRow::CommentOmitted {
                count: omitted_count,
                depth: omitted_guide.depth(),
                guide: omitted_guide,
            });
        }

        for (idx, comment) in visible_comments.iter().take(display_count).enumerate() {
            let item_guide = comment_guide.child(idx + 1 == display_count);
            rows.push(TreeRow::CommentItem {
                comment,
                depth: item_guide.depth(),
                guide: item_guide,
            });
        }
    }

    if let Some(ref ts) = node.deliverables {
        let deliverable_count = ts.data.len();
        let deliverable_guide = guide.child(!has_children);
        rows.push(TreeRow::DeliverableHeader {
            count: deliverable_count,
            depth: deliverable_guide.depth(),
            guide: deliverable_guide.clone(),
        });
        for (idx, deliverable) in ts.data.iter().enumerate() {
            let item_guide = deliverable_guide.child(idx + 1 == deliverable_count);
            rows.push(TreeRow::DeliverableItem {
                deliverable,
                depth: item_guide.depth(),
                guide: item_guide,
            });
        }
    }

    if let Some(ref ts) = node.children {
        let child_count = ts.data.len();
        for (idx, child) in ts.data.iter().enumerate() {
            let child_guide = guide.child(idx + 1 == child_count);
            flatten_node(child, view, child_guide, rows);
        }
    }
}

fn toggle_at<S: GoalItemAccessor>(
    node: &mut GoalNodeInner<S>,
    target: usize,
    idx: &mut usize,
    view: CommentView,
) -> bool {
    if *idx == target {
        node.expanded = !node.expanded;
        return true;
    }
    *idx += 1;

    if node.expanded {
        *idx += 1; // detail row
        *idx += comment_row_count(&node.comments, node.comment_display_limit, view);
        if let Some(ts) = &node.deliverables {
            *idx += 1 + ts.data.len();
        }
        if let Some(ts) = &mut node.children {
            for child in &mut ts.data {
                if toggle_at(child, target, idx, view) {
                    return true;
                }
            }
        }
    }

    false
}

fn set_children_at<S: GoalItemAccessor>(
    node: &mut GoalNodeInner<S>,
    target: usize,
    idx: &mut usize,
    children: &mut Option<Vec<GoalChildItem>>,
    view: CommentView,
) -> bool {
    if *idx == target {
        if let Some(items) = children.take() {
            let child_nodes = items
                .into_iter()
                .map(|item| GoalChildNode {
                    node: item,
                    children: None,
                    comments: None,
                    deliverables: None,
                    expanded: false,
                    comment_display_limit: INITIAL_COMMENT_DISPLAY_LIMIT,
                })
                .collect();
            node.children = Some(Timestamped::now(child_nodes));
        }
        return true;
    }
    *idx += 1;

    if node.expanded {
        *idx += 1; // detail row
        *idx += comment_row_count(&node.comments, node.comment_display_limit, view);
        if let Some(ts) = &node.deliverables {
            *idx += 1 + ts.data.len();
        }
        if let Some(ts) = &mut node.children {
            for child in &mut ts.data {
                if set_children_at(child, target, idx, children, view) {
                    return true;
                }
            }
        }
    }

    false
}

fn set_comments_at<S: GoalItemAccessor>(
    node: &mut GoalNodeInner<S>,
    target: usize,
    idx: &mut usize,
    comments: &mut Option<Vec<Comment>>,
    view: CommentView,
) -> bool {
    if *idx == target {
        if let Some(items) = comments.take() {
            node.comments = Some(Timestamped::now(items));
        }
        return true;
    }
    *idx += 1;

    if node.expanded {
        *idx += 1; // detail row
        *idx += comment_row_count(&node.comments, node.comment_display_limit, view);
        if let Some(ts) = &node.deliverables {
            *idx += 1 + ts.data.len();
        }
        if let Some(ts) = &mut node.children {
            for child in &mut ts.data {
                if set_comments_at(child, target, idx, comments, view) {
                    return true;
                }
            }
        }
    }

    false
}

fn set_deliverables_at<S: GoalItemAccessor>(
    node: &mut GoalNodeInner<S>,
    target: usize,
    idx: &mut usize,
    deliverables: &mut Option<Vec<Deliverable>>,
    view: CommentView,
) -> bool {
    if *idx == target {
        if let Some(items) = deliverables.take() {
            node.deliverables = Some(Timestamped::now(items));
        }
        return true;
    }
    *idx += 1;

    if node.expanded {
        *idx += 1; // detail row
        *idx += comment_row_count(&node.comments, node.comment_display_limit, view);
        if let Some(ts) = &node.deliverables {
            *idx += 1 + ts.data.len();
        }
        if let Some(ts) = &mut node.children {
            for child in &mut ts.data {
                if set_deliverables_at(child, target, idx, deliverables, view) {
                    return true;
                }
            }
        }
    }

    false
}

fn increase_comment_limit_at<S: GoalItemAccessor>(
    node: &mut GoalNodeInner<S>,
    target: usize,
    idx: &mut usize,
    view: CommentView,
) -> bool {
    *idx += 1;

    if node.expanded {
        *idx += 1; // detail row

        // Calculate comment section indices
        let comment_start = *idx;
        let comment_rows = comment_row_count(&node.comments, node.comment_display_limit, view);
        let comment_end = comment_start + comment_rows;

        // If target is within comment section, increase this node's limit
        if target >= comment_start && target < comment_end {
            node.comment_display_limit += COMMENT_DISPLAY_INCREMENT;
            return true;
        }

        *idx += comment_rows;

        if let Some(ts) = &node.deliverables {
            *idx += 1 + ts.data.len();
        }
        if let Some(ts) = &mut node.children {
            for child in &mut ts.data {
                if increase_comment_limit_at(child, target, idx, view) {
                    return true;
                }
            }
        }
    }

    false
}

/// Set comments on a node matched by goal_id, without relying on flat indices.
fn set_comments_by_id<S: GoalItemAccessor>(
    node: &mut GoalNodeInner<S>,
    goal_id: &str,
    comments: &mut Option<Vec<Comment>>,
) -> bool {
    if node.node.id() == goal_id {
        if let Some(items) = comments.take() {
            node.comments = Some(Timestamped::now(items));
        }
        return true;
    }
    if let Some(ts) = &mut node.children {
        for child in &mut ts.data {
            if set_comments_by_id(child, goal_id, comments) {
                return true;
            }
        }
    }
    false
}

/// Set deliverables on a node matched by goal_id, without relying on flat indices.
fn set_deliverables_by_id<S: GoalItemAccessor>(
    node: &mut GoalNodeInner<S>,
    goal_id: &str,
    deliverables: &mut Option<Vec<Deliverable>>,
) -> bool {
    if node.node.id() == goal_id {
        if let Some(items) = deliverables.take() {
            node.deliverables = Some(Timestamped::now(items));
        }
        return true;
    }
    if let Some(ts) = &mut node.children {
        for child in &mut ts.data {
            if set_deliverables_by_id(child, goal_id, deliverables) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Today's goals tree building helpers
// ---------------------------------------------------------------------------

/// Build a GoalRootNode from a TodaysGoalNode, including all its children recursively
fn build_goal_node_from_todays(
    node: &TodaysGoalNode,
    node_map: &HashMap<String, TodaysGoalNode>,
    children_map: &HashMap<Option<String>, Vec<String>>,
) -> GoalRootNode {
    use crate::dbg_log;

    // Convert TodaysGoalNode to GoalTreeItem
    let tree_item = GoalTreeItem {
        id: node.id.clone(),
        parent_id: node.parent_id.clone(),
        title: node.title.clone(),
        status: node.parsed_status(),
        order_no: node.order_no,
        is_completed: node.is_completed(),
        has_children: !node.is_leaf,
        owner: node.owner.clone(),
    };

    // Build children recursively
    let children = if let Some(child_ids) = children_map.get(&Some(node.id.clone())) {
        let child_nodes: Vec<GoalChildNode> = child_ids
            .iter()
            .filter_map(|child_id| {
                let child_node = node_map.get(child_id)?;
                Some(build_child_node_from_todays(
                    child_node,
                    node_map,
                    children_map,
                ))
            })
            .collect();

        if !child_nodes.is_empty() {
            dbg_log!(
                "  Node '{}' has {} children (pre-loaded)",
                node.title,
                child_nodes.len()
            );
            Some(Timestamped::now(child_nodes))
        } else {
            None
        }
    } else {
        None
    };

    GoalRootNode {
        node: tree_item,
        children,
        comments: None,
        deliverables: None,
        expanded: true, // Always expanded since we have the complete tree
        comment_display_limit: INITIAL_COMMENT_DISPLAY_LIMIT,
    }
}

/// Build a GoalChildNode from a TodaysGoalNode, including all its children recursively
fn build_child_node_from_todays(
    node: &TodaysGoalNode,
    node_map: &HashMap<String, TodaysGoalNode>,
    children_map: &HashMap<Option<String>, Vec<String>>,
) -> GoalChildNode {
    // Convert TodaysGoalNode to GoalChildItem
    let child_item = GoalChildItem {
        id: node.id.clone(),
        title: node.title.clone(),
        description: None, // TodaysGoalNode doesn't include description
        parent_id: node.parent_id.clone(),
        status: node.parsed_status(),
        is_completed: node.is_completed(),
        has_children: !node.is_leaf,
        order_no: node.order_no,
        owner: node.owner.clone(),
    };

    // Build children recursively
    let children = if let Some(child_ids) = children_map.get(&Some(node.id.clone())) {
        let child_nodes: Vec<GoalChildNode> = child_ids
            .iter()
            .filter_map(|child_id| {
                let child_node = node_map.get(child_id)?;
                Some(build_child_node_from_todays(
                    child_node,
                    node_map,
                    children_map,
                ))
            })
            .collect();

        if !child_nodes.is_empty() {
            Some(Timestamped::now(child_nodes))
        } else {
            None
        }
    } else {
        None
    };

    GoalChildNode {
        node: child_item,
        children,
        comments: None,
        deliverables: None,
        expanded: true, // Always expanded since we have the complete tree
        comment_display_limit: INITIAL_COMMENT_DISPLAY_LIMIT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{CommentAuthor, TodaysGoalNode};

    fn mk_comment(id: &str, resolved: bool) -> Comment {
        Comment {
            id: id.to_string(),
            content: format!("comment {id}"),
            commentable_type: "objective".to_string(),
            commentable_id: "g1".to_string(),
            parent_id: None,
            author: CommentAuthor {
                id: "u1".to_string(),
                name: "user".to_string(),
                is_ai_agent: false,
            },
            reply_count: 0,
            resolved_at: resolved.then(|| "2020-01-01T00:00:00Z".to_string()),
            created_at: format!("2020-01-01T00:00:0{id}Z"),
            updated_at: "2020-01-01T00:00:00Z".to_string(),
        }
    }

    fn mk_tree(comments: Vec<Comment>, limit: usize, view: CommentView) -> GoalTree {
        let node = GoalRootNode {
            node: GoalTreeItem {
                id: "g1".to_string(),
                parent_id: None,
                title: "G1".to_string(),
                status: None,
                order_no: 0.0,
                is_completed: false,
                has_children: false,
                owner: None,
            },
            children: None,
            comments: Some(Timestamped::now(comments)),
            deliverables: None,
            expanded: true,
            comment_display_limit: limit,
        };
        GoalTree {
            roots: vec![node],
            cursor: 0,
            scroll_offset: 0,
            is_required_to_fetch: true,
            comment_view: view,
        }
    }

    fn mk_tree_item(id: &str, parent_id: Option<&str>, is_completed: bool) -> GoalTreeItem {
        GoalTreeItem {
            id: id.to_string(),
            parent_id: parent_id.map(ToString::to_string),
            title: format!("Goal {id}"),
            status: None,
            order_no: 0.0,
            is_completed,
            has_children: false,
            owner: None,
        }
    }

    fn mk_today_node(id: &str, parent_id: Option<&str>, order_no: f64) -> TodaysGoalNode {
        TodaysGoalNode {
            id: id.to_string(),
            parent_id: parent_id.map(ToString::to_string),
            depth: 0,
            title: format!("Goal {id}"),
            status: "NONE".to_string(),
            completed_at: None,
            order_no,
            is_leaf: false,
            has_recurring: false,
            is_recurring: false,
            kind: "goal".to_string(),
            execution: None,
            owner: None,
            is_direct_assignment: true,
            is_ai_running: None,
        }
    }

    /// flatten が実際に生成するコメント行数。
    fn flattened_comment_rows(tree: &GoalTree) -> usize {
        tree.flatten()
            .iter()
            .filter(|r| {
                matches!(
                    r,
                    TreeRow::CommentHeader { .. }
                        | TreeRow::CommentOmitted { .. }
                        | TreeRow::CommentItem { .. }
                )
            })
            .count()
    }

    /// flatten のコメント行数と、索引計算に使う comment_row_count が
    /// 全モード・上限で一致すること（不一致はカーソル計算を壊す）。
    #[test]
    fn comment_row_count_matches_flatten_all_views() {
        let comments = vec![
            mk_comment("1", false),
            mk_comment("2", true),
            mk_comment("3", false),
            mk_comment("4", true),
        ];
        for view in [
            CommentView::Hidden,
            CommentView::Unresolved,
            CommentView::All,
        ] {
            for limit in [1usize, 2, 20] {
                let tree = mk_tree(comments.clone(), limit, view);
                let expected = comment_row_count(&tree.roots[0].comments, limit, view);
                assert_eq!(
                    flattened_comment_rows(&tree),
                    expected,
                    "mismatch for view={view:?} limit={limit}"
                );
            }
        }
    }

    #[test]
    fn hidden_view_emits_no_comment_rows() {
        let tree = mk_tree(vec![mk_comment("1", false)], 20, CommentView::Hidden);
        assert_eq!(flattened_comment_rows(&tree), 0);
    }

    #[test]
    fn unresolved_view_excludes_resolved_but_all_includes() {
        let comments = vec![mk_comment("1", false), mk_comment("2", true)];
        // unresolved: header + 1 item = 2
        let unresolved = mk_tree(comments.clone(), 20, CommentView::Unresolved);
        assert_eq!(flattened_comment_rows(&unresolved), 2);
        // all: header + 2 items = 3
        let all = mk_tree(comments, 20, CommentView::All);
        assert_eq!(flattened_comment_rows(&all), 3);
    }

    #[test]
    fn omitted_row_appears_over_limit() {
        let comments = vec![
            mk_comment("1", false),
            mk_comment("2", false),
            mk_comment("3", false),
        ];
        // limit 2: header + 2 items + omitted = 4
        let tree = mk_tree(comments, 2, CommentView::Unresolved);
        assert_eq!(flattened_comment_rows(&tree), 4);
    }

    #[test]
    fn cycle_clamps_cursor_when_rows_shrink() {
        // All モードで全行を出し、最終行にカーソルを置く。
        let comments = vec![mk_comment("1", true), mk_comment("2", true)];
        let mut tree = mk_tree(comments, 20, CommentView::All);
        tree.cursor = tree.flatten().len() - 1;
        // All → Hidden に切り替えるとコメント行が消え、行数が減る。
        tree.cycle_comment_view();
        assert!(
            tree.cursor < tree.flatten().len(),
            "cursor must stay in range after view change"
        );
    }

    #[test]
    fn todays_tree_treats_nodes_with_missing_parent_as_visible_roots() {
        let nodes = vec![
            mk_today_node("child", Some("parent-outside-response"), 2.0),
            mk_today_node("grandchild", Some("child"), 1.0),
        ];

        let tree = GoalTree::from_todays_goal_nodes(nodes);

        assert_eq!(tree.roots.len(), 1);
        assert_eq!(tree.roots[0].node.id, "child");
        let goal_rows = tree
            .flatten()
            .into_iter()
            .filter_map(|row| match row {
                TreeRow::Goal { goal_id, depth, .. } => Some((goal_id.to_string(), depth)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            goal_rows,
            vec![("child".to_string(), 0), ("grandchild".to_string(), 1)]
        );
    }

    #[test]
    fn active_goal_tree_drops_completed_items_even_if_api_leaks_them() {
        let tree = GoalTree::from_tree_items(vec![
            mk_tree_item("done-child", Some("missing-parent"), true),
            mk_tree_item("active-root", None, false),
        ]);

        let goal_ids = tree
            .flatten()
            .into_iter()
            .filter_map(|row| match row {
                TreeRow::Goal { goal_id, .. } => Some(goal_id.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(goal_ids, vec!["active-root".to_string()]);
    }
}
