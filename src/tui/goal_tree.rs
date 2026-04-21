use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use crate::api::{Comment, Deliverable, GoalChildItem, GoalStatus, GoalTreeItem, MemberId, Owner};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const INITIAL_COMMENT_DISPLAY_LIMIT: usize = 20;
const COMMENT_DISPLAY_INCREMENT: usize = 20;

// ---------------------------------------------------------------------------
// Timestamped wrapper
// ---------------------------------------------------------------------------

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

pub struct GoalTree {
    pub roots: Vec<GoalRootNode>,
    pub cursor: usize,
    pub scroll_offset: usize,
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
        }
    }

    pub fn from_tree_items(items: Vec<GoalTreeItem>) -> Self {
        let roots = items
            .into_iter()
            .map(|item| GoalRootNode {
                node: item,
                children: None,
                comments: None,
                deliverables: None,
                expanded: false,
                comment_display_limit: INITIAL_COMMENT_DISPLAY_LIMIT,
            })
            .collect();
        GoalTree {
            roots,
            cursor: 0,
            scroll_offset: 0,
        }
    }

    /// Create a filtered tree containing only goals owned by the specified user
    /// and all ancestors needed to show the path from root to owned goals.
    /// Auto-expands goals owned by the user.
    pub fn from_tree_items_filtered(items: Vec<GoalTreeItem>, member_id: &MemberId) -> Self {
        if items.is_empty() {
            return Self::empty();
        }

        // Phase 1: Build lookup map
        let goal_map: HashMap<String, &GoalTreeItem> =
            items.iter().map(|item| (item.id.clone(), item)).collect();

        // Phase 2: Find owned goals and collect all needed IDs (owned + ancestors)
        let mut needed_ids: HashSet<String> = HashSet::new();

        for item in &items {
            if let Some(owner) = &item.owner
                && &owner.organization_member_id == member_id
            {
                // Add this goal and all its ancestors
                add_goal_and_ancestors(&item.id, &goal_map, &mut needed_ids);
            }
        }

        // If no goals are owned by the user, return empty tree
        if needed_ids.is_empty() {
            return Self::empty();
        }

        // Phase 3: Filter and build tree
        let roots = items
            .into_iter()
            .filter(|item| needed_ids.contains(&item.id))
            .map(|item| {
                let is_owned = item
                    .owner
                    .as_ref()
                    .map(|o| &o.organization_member_id == member_id)
                    .unwrap_or(false);

                GoalRootNode {
                    node: item,
                    children: None,
                    comments: None,
                    deliverables: None,
                    expanded: is_owned, // Auto-expand owned goals
                    comment_display_limit: INITIAL_COMMENT_DISPLAY_LIMIT,
                }
            })
            .collect();

        GoalTree {
            roots,
            cursor: 0,
            scroll_offset: 0,
        }
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

    /// Insert fetched children with filtering applied for execution view.
    /// Only includes children owned by member_id or that are ancestors of owned goals.
    pub fn set_children_at_cursor_filtered(
        &mut self,
        children: Vec<GoalChildItem>,
        member_id: &MemberId,
    ) {
        // Build lookup map for children
        let child_map: HashMap<String, &GoalChildItem> = children
            .iter()
            .map(|item| (item.id.clone(), item))
            .collect();

        // Find needed IDs (owned + ancestors of owned)
        let mut needed_ids: HashSet<String> = HashSet::new();

        for child in &children {
            if let Some(owner) = &child.owner
                && &owner.organization_member_id == member_id
            {
                add_child_and_ancestors(&child.id, &child_map, &mut needed_ids);
            }
        }

        // Filter children
        let filtered_children: Vec<GoalChildItem> = children
            .into_iter()
            .filter(|item| needed_ids.contains(&item.id))
            .collect();

        // Use standard method to set filtered children
        let target = self.cursor;
        let mut idx = 0;
        let mut filtered = Some((filtered_children, member_id.clone()));
        for root in &mut self.roots {
            if set_children_at_filtered(root, target, &mut idx, &mut filtered) {
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

fn set_children_at_filtered<S: GoalItemAccessor>(
    node: &mut GoalNodeInner<S>,
    target: usize,
    idx: &mut usize,
    children_with_member_id: &mut Option<(Vec<GoalChildItem>, MemberId)>,
) -> bool {
    if *idx == target {
        if let Some((items, member_id)) = children_with_member_id.take() {
            let child_nodes = items
                .into_iter()
                .map(|item| {
                    let is_owned = item
                        .owner
                        .as_ref()
                        .map(|o| o.organization_member_id == member_id)
                        .unwrap_or(false);

                    GoalChildNode {
                        node: item,
                        children: None,
                        comments: None,
                        deliverables: None,
                        expanded: is_owned, // Auto-expand owned goals
                        comment_display_limit: INITIAL_COMMENT_DISPLAY_LIMIT,
                    }
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
                if set_children_at_filtered(child, target, idx, children_with_member_id) {
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

// ---------------------------------------------------------------------------
// Filtering helpers
// ---------------------------------------------------------------------------

/// Recursively add a goal and all its ancestors to the needed_ids set
fn add_goal_and_ancestors(
    goal_id: &str,
    goal_map: &HashMap<String, &GoalTreeItem>,
    needed_ids: &mut HashSet<String>,
) {
    // Add this goal
    needed_ids.insert(goal_id.to_string());

    // Recursively add parent if it exists
    if let Some(goal) = goal_map.get(goal_id)
        && let Some(parent_id) = &goal.parent_id
    {
        add_goal_and_ancestors(parent_id, goal_map, needed_ids);
    }
}

/// Recursively add a child goal and all its ancestors to the needed_ids set
fn add_child_and_ancestors(
    child_id: &str,
    child_map: &HashMap<String, &GoalChildItem>,
    needed_ids: &mut HashSet<String>,
) {
    // Add this child
    needed_ids.insert(child_id.to_string());

    // Recursively add parent if it exists
    if let Some(child) = child_map.get(child_id)
        && let Some(parent_id) = &child.parent_id
    {
        add_child_and_ancestors(parent_id, child_map, needed_ids);
    }
}
