use chrono::{DateTime, Utc};

use crate::api::{Comment, Deliverable, GoalChildItem, GoalStatus, GoalTreeItem, Owner};

// ---------------------------------------------------------------------------
// Timestamped wrapper
// ---------------------------------------------------------------------------

pub struct Timestamped<T> {
    pub data: T,
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
        rows.push(TreeRow::CommentHeader {
            count: ts.data.len(),
            depth: child_depth,
        });
        for comment in &ts.data {
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
        if let Some(ts) = &node.comments {
            *idx += 1 + ts.data.len();
        }
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
                })
                .collect();
            node.children = Some(Timestamped::now(child_nodes));
        }
        return true;
    }
    *idx += 1;

    if node.expanded {
        *idx += 1; // detail row
        if let Some(ts) = &node.comments {
            *idx += 1 + ts.data.len();
        }
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
