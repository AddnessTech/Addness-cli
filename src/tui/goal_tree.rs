use chrono::{DateTime, Utc};

use crate::api::{
    Comment, CommentAuthor, Deliverable, DeliverableType, GoalChildItem, GoalStatus, GoalTreeItem,
    Owner,
};

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
    fn owner(&self) -> Option<&Owner>;
}

impl GoalItemAccessor for GoalTreeItem {
    fn id(&self) -> &str { &self.id }
    fn title(&self) -> &str { &self.title }
    fn description(&self) -> Option<&str> { None }
    fn status(&self) -> Option<&GoalStatus> { self.status.as_ref() }
    fn is_completed(&self) -> bool { self.is_completed }
    fn owner(&self) -> Option<&Owner> { self.owner.as_ref() }
}

impl GoalItemAccessor for GoalChildItem {
    fn id(&self) -> &str { &self.id }
    fn title(&self) -> &str { &self.title }
    fn description(&self) -> Option<&str> { self.description.as_deref() }
    fn status(&self) -> Option<&GoalStatus> { self.status.as_ref() }
    fn is_completed(&self) -> bool { self.is_completed }
    fn owner(&self) -> Option<&Owner> { self.owner.as_ref() }
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
    pub root: Timestamped<GoalRootNode>,
    pub cursor: usize,
    pub scroll_offset: usize,
}

// ---------------------------------------------------------------------------
// TreeRow – one flattened row for rendering
// ---------------------------------------------------------------------------

pub enum TreeRow<'a> {
    Goal {
        title: &'a str,
        status: Option<&'a GoalStatus>,
        owner_name: Option<&'a str>,
        is_completed: bool,
        expanded: bool,
        depth: usize,
    },
    Detail {
        status: Option<&'a GoalStatus>,
        owner_name: Option<&'a str>,
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
// Tree operations
// ---------------------------------------------------------------------------

impl GoalTree {
    pub fn flatten(&self) -> Vec<TreeRow<'_>> {
        let mut rows = Vec::new();
        flatten_node(&self.root.data, 0, &mut rows);
        rows
    }

    pub fn toggle_expand(&mut self) {
        let rows = self.flatten();
        if let Some(TreeRow::Goal { .. }) = rows.get(self.cursor) {
            let mut idx = 0;
            toggle_at(&mut self.root.data, self.cursor, &mut idx);
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
// Generic flatten / toggle helpers (no duplication between Root and Child)
// ---------------------------------------------------------------------------

fn flatten_node<'a, S: GoalItemAccessor>(
    node: &'a GoalNodeInner<S>,
    depth: usize,
    rows: &mut Vec<TreeRow<'a>>,
) {
    rows.push(TreeRow::Goal {
        title: node.node.title(),
        status: node.node.status(),
        owner_name: node.node.owner().map(|o| o.name.as_str()),
        is_completed: node.node.is_completed(),
        expanded: node.expanded,
        depth,
    });

    if !node.expanded {
        return;
    }

    let child_depth = depth + 1;

    // Detail row (always available from node)
    rows.push(TreeRow::Detail {
        status: node.node.status(),
        owner_name: node.node.owner().map(|o| o.name.as_str()),
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
        // Detail row (always present when expanded)
        *idx += 1;
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

// ---------------------------------------------------------------------------
// Mock data builder
// ---------------------------------------------------------------------------

impl GoalTree {
    pub fn mock() -> Self {
        let root = GoalRootNode {
            node: GoalTreeItem {
                id: "goal-001".into(),
                parent_id: None,
                title: "Company Strategy".into(),
                status: Some(GoalStatus::Active),
                order_no: 1.0,
                is_completed: false,
                has_children: true,
                owner: Some(Owner {
                    id: "user-001".into(),
                    name: "Alice".into(),
                }),
            },
            comments: Some(Timestamped::now(vec![
                Comment {
                    id: "comment-001".into(),
                    content: "Great progress so far".into(),
                    commentable_type: "Objective".into(),
                    commentable_id: "goal-001".into(),
                    parent_id: None,
                    author: CommentAuthor {
                        id: "user-001".into(),
                        name: "Alice".into(),
                        is_ai_agent: false,
                    },
                    reply_count: 0,
                    resolved_at: None,
                    created_at: "2025-04-01T10:30:00Z".into(),
                    updated_at: "2025-04-01T10:30:00Z".into(),
                },
                Comment {
                    id: "comment-002".into(),
                    content: "Need to revisit Q3 targets".into(),
                    commentable_type: "Objective".into(),
                    commentable_id: "goal-001".into(),
                    parent_id: None,
                    author: CommentAuthor {
                        id: "user-002".into(),
                        name: "Bob".into(),
                        is_ai_agent: false,
                    },
                    reply_count: 0,
                    resolved_at: None,
                    created_at: "2025-04-02T14:15:00Z".into(),
                    updated_at: "2025-04-02T14:15:00Z".into(),
                },
            ])),
            deliverables: Some(Timestamped::now(vec![Deliverable {
                id: "del-001".into(),
                display_name: "Strategy Document".into(),
                node_type: DeliverableType::Document,
                content: None,
                link_url: None,
                file_name: Some("strategy-2025.pdf".into()),
                objective_id: "goal-001".into(),
                parent_deliverable_id: None,
                order_no: 1.0,
                depth: 0,
                is_root: true,
                has_children: false,
                children_count: 0,
            }])),
            children: Some(Timestamped::now(vec![
                // Child: Q1 Goals – collapsed
                GoalChildNode {
                    node: GoalChildItem {
                        id: "goal-002".into(),
                        title: "Q1 Goals".into(),
                        description: Some("First quarter objectives".into()),
                        parent_id: Some("goal-001".into()),
                        status: Some(GoalStatus::Completed),
                        is_completed: true,
                        has_children: true,
                        order_no: 1.0,
                        owner: Some(Owner {
                            id: "user-001".into(),
                            name: "Alice".into(),
                        }),
                    },
                    children: None,
                    comments: None,
                    deliverables: None,
                    expanded: false,
                },
                // Child: Q2 Goals – expanded with children
                GoalChildNode {
                    node: GoalChildItem {
                        id: "goal-003".into(),
                        title: "Q2 Goals".into(),
                        description: Some("Second quarter objectives".into()),
                        parent_id: Some("goal-001".into()),
                        status: Some(GoalStatus::InProgress),
                        is_completed: false,
                        has_children: true,
                        order_no: 2.0,
                        owner: Some(Owner {
                            id: "user-002".into(),
                            name: "Bob".into(),
                        }),
                    },
                    children: Some(Timestamped::now(vec![
                        GoalChildNode {
                            node: GoalChildItem {
                                id: "goal-004".into(),
                                title: "Revenue Target".into(),
                                description: Some("Achieve $1M ARR by end of Q2".into()),
                                parent_id: Some("goal-003".into()),
                                status: Some(GoalStatus::InProgress),
                                is_completed: false,
                                has_children: false,
                                order_no: 1.0,
                                owner: Some(Owner {
                                    id: "user-003".into(),
                                    name: "Charlie".into(),
                                }),
                            },
                            children: Some(Timestamped::now(vec![
                                GoalChildNode {
                                    node: GoalChildItem {
                                        id: "goal-006".into(),
                                        parent_id: None,
                                        title: "Personal Development".into(),
                                        description: None,
                                        status: Some(GoalStatus::Active),
                                        order_no: 2.0,
                                        is_completed: false,
                                        has_children: true,
                                        owner: Some(Owner {
                                            id: "user-002".into(),
                                            name: "Bob".into(),
                                        }),
                                    },
                                    children: None,
                                    comments: None,
                                    deliverables: None,
                                    expanded: false,
                                },
                                GoalChildNode {
                                    node: GoalChildItem {
                                        id: "goal-007".into(),
                                        parent_id: None,
                                        title: "Infrastructure Upgrade".into(),
                                        description: Some(
                                            "Migrate to new cloud provider".into(),
                                        ),
                                        status: Some(GoalStatus::InProgress),
                                        order_no: 3.0,
                                        is_completed: false,
                                        has_children: false,
                                        owner: Some(Owner {
                                            id: "user-003".into(),
                                            name: "Charlie".into(),
                                        }),
                                    },
                                    children: None,
                                    comments: None,
                                    deliverables: None,
                                    expanded: false,
                                },
                            ])),
                            comments: None,
                            deliverables: None,
                            expanded: false,
                        },
                        GoalChildNode {
                            node: GoalChildItem {
                                id: "goal-005".into(),
                                title: "Customer Acquisition".into(),
                                description: Some("Acquire 500 new customers".into()),
                                parent_id: Some("goal-003".into()),
                                status: Some(GoalStatus::Active),
                                is_completed: false,
                                has_children: false,
                                order_no: 2.0,
                                owner: Some(Owner {
                                    id: "user-004".into(),
                                    name: "Diana".into(),
                                }),
                            },
                            children: None,
                            comments: None,
                            deliverables: None,
                            expanded: false,
                        },
                    ])),
                    comments: None,
                    deliverables: None,
                    expanded: true,
                },
            ])),
            expanded: true,
        };

        GoalTree {
            root: Timestamped::now(root),
            cursor: 0,
            scroll_offset: 0,
        }
    }
}
