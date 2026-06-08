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
        depth: usize,
    },
    Detail {
        status: Option<&'a GoalStatus>,
        owner_name: Option<&'a str>,
        is_completed: bool,
        description: Option<&'a str>,
        depth: usize,
    },
    CommentHeader {
        count: usize,
        depth: usize,
    },
    CommentOmitted {
        count: usize,
        depth: usize,
    },
    CommentItem {
        comment: &'a Comment,
        depth: usize,
    },
    DeliverableHeader {
        count: usize,
        depth: usize,
    },
    DeliverableItem {
        deliverable: &'a Deliverable,
        depth: usize,
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
        }
    }

    pub fn from_tree_items(items: Vec<GoalTreeItem>) -> Self {
        use std::collections::{HashMap, HashSet};

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

        // Build root nodes (nodes with no parent)
        let root_ids = children_map.get(&None).cloned().unwrap_or_default();

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
        for root in &self.roots {
            flatten_node(root, 0, &mut rows);
        }
        rows
    }

    pub fn toggle_expand(&mut self) {
        let rows = self.flatten();
        if let Some(TreeRow::Goal { .. }) = rows.get(self.cursor) {
            let mut idx = 0;
            for root in &mut self.roots {
                if toggle_at(root, self.cursor, &mut idx) {
                    return;
                }
            }
        }
    }

    /// Insert fetched children into the Goal node at the current cursor position.
    pub fn set_children_at_cursor(&mut self, children: Vec<GoalChildItem>) {
        let target = self.cursor;
        let mut idx = 0;
        let mut children = Some(children);
        for root in &mut self.roots {
            if set_children_at(root, target, &mut idx, &mut children) {
                return;
            }
        }
    }

    /// Insert fetched comments into the Goal node at the current cursor position.
    pub fn set_comments_at_cursor(&mut self, comments: Vec<Comment>) {
        let target = self.cursor;
        let mut idx = 0;
        let mut comments = Some(comments);
        for root in &mut self.roots {
            if set_comments_at(root, target, &mut idx, &mut comments) {
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
        let target = self.cursor;
        let mut idx = 0;
        let mut deliverables = Some(deliverables);
        for root in &mut self.roots {
            if set_deliverables_at(root, target, &mut idx, &mut deliverables) {
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
        let target = self.cursor;
        let mut idx = 0;
        for root in &mut self.roots {
            if increase_comment_limit_at(root, target, &mut idx) {
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
/// Only counts unresolved comments (resolved_at is None).
/// Includes header, up to `limit` most recent items, and omitted row if total > limit.
fn comment_row_count(comments: &Option<Timestamped<Vec<Comment>>>, limit: usize) -> usize {
    match comments {
        Some(ts) => {
            let unresolved_count = ts.data.iter().filter(|c| c.resolved_at.is_none()).count();
            let displayed = unresolved_count.min(limit);
            let has_omitted = unresolved_count > limit;
            1 + displayed + if has_omitted { 1 } else { 0 } // header + items + omitted row
        }
        None => 0,
    }
}

fn flatten_node<'a, S: GoalItemAccessor>(
    node: &'a GoalNodeInner<S>,
    depth: usize,
    rows: &mut Vec<TreeRow<'a>>,
) {
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
        depth,
    });

    if !node.expanded {
        return;
    }

    let child_depth = depth + 1;

    rows.push(TreeRow::Detail {
        status: node.node.status(),
        owner_name: node.node.owner().map(|o| o.name.as_str()),
        is_completed: node.node.is_completed(),
        description: node.node.description(),
        depth: child_depth,
    });

    if let Some(ref ts) = node.comments {
        // Filter to only unresolved comments (resolved_at is None)
        let mut unresolved_comments: Vec<&Comment> =
            ts.data.iter().filter(|c| c.resolved_at.is_none()).collect();

        let total_unresolved = unresolved_comments.len();

        rows.push(TreeRow::CommentHeader {
            count: total_unresolved,
            depth: child_depth,
        });

        // Sort by created_at descending (newest first)
        unresolved_comments.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        let display_count = total_unresolved.min(node.comment_display_limit);
        let omitted_count = total_unresolved.saturating_sub(node.comment_display_limit);

        if omitted_count > 0 {
            rows.push(TreeRow::CommentOmitted {
                count: omitted_count,
                depth: child_depth + 1,
            });
        }

        for comment in unresolved_comments.iter().take(display_count) {
            rows.push(TreeRow::CommentItem {
                comment,
                depth: child_depth + 1,
            });
        }
    }

    if let Some(ref ts) = node.deliverables {
        rows.push(TreeRow::DeliverableHeader {
            count: ts.data.len(),
            depth: child_depth,
        });
        for deliverable in &ts.data {
            rows.push(TreeRow::DeliverableItem {
                deliverable,
                depth: child_depth + 1,
            });
        }
    }

    if let Some(ref ts) = node.children {
        for child in &ts.data {
            flatten_node(child, depth + 1, rows);
        }
    }
}

fn toggle_at<S: GoalItemAccessor>(
    node: &mut GoalNodeInner<S>,
    target: usize,
    idx: &mut usize,
) -> bool {
    if *idx == target {
        node.expanded = !node.expanded;
        return true;
    }
    *idx += 1;

    if node.expanded {
        *idx += 1; // detail row
        *idx += comment_row_count(&node.comments, node.comment_display_limit);
        if let Some(ts) = &node.deliverables {
            *idx += 1 + ts.data.len();
        }
        if let Some(ts) = &mut node.children {
            for child in &mut ts.data {
                if toggle_at(child, target, idx) {
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
        *idx += comment_row_count(&node.comments, node.comment_display_limit);
        if let Some(ts) = &node.deliverables {
            *idx += 1 + ts.data.len();
        }
        if let Some(ts) = &mut node.children {
            for child in &mut ts.data {
                if set_children_at(child, target, idx, children) {
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
        *idx += comment_row_count(&node.comments, node.comment_display_limit);
        if let Some(ts) = &node.deliverables {
            *idx += 1 + ts.data.len();
        }
        if let Some(ts) = &mut node.children {
            for child in &mut ts.data {
                if set_comments_at(child, target, idx, comments) {
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
        *idx += comment_row_count(&node.comments, node.comment_display_limit);
        if let Some(ts) = &node.deliverables {
            *idx += 1 + ts.data.len();
        }
        if let Some(ts) = &mut node.children {
            for child in &mut ts.data {
                if set_deliverables_at(child, target, idx, deliverables) {
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
) -> bool {
    *idx += 1;

    if node.expanded {
        *idx += 1; // detail row

        // Calculate comment section indices
        let comment_start = *idx;
        let comment_rows = comment_row_count(&node.comments, node.comment_display_limit);
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
                if increase_comment_limit_at(child, target, idx) {
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
