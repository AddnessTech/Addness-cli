use chrono::{DateTime, Utc};

use crate::api::{
    Comment, CommentAuthor, Deliverable, DeliverableType, Goal, GoalChildItem, GoalStatus,
    GoalTreeItem, Owner,
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
// GoalSummary – unified accessor over Root vs Child items
// ---------------------------------------------------------------------------

pub enum GoalSummary {
    Root(GoalTreeItem),
    Child(GoalChildItem),
}

impl GoalSummary {
    pub fn id(&self) -> &str {
        match self {
            GoalSummary::Root(r) => &r.id,
            GoalSummary::Child(c) => &c.id,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            GoalSummary::Root(r) => &r.title,
            GoalSummary::Child(c) => &c.title,
        }
    }

    pub fn status(&self) -> Option<&GoalStatus> {
        match self {
            GoalSummary::Root(r) => r.status.as_ref(),
            GoalSummary::Child(c) => c.status.as_ref(),
        }
    }

    pub fn has_children(&self) -> bool {
        match self {
            GoalSummary::Root(r) => r.has_children,
            GoalSummary::Child(c) => c.has_children,
        }
    }

    pub fn is_completed(&self) -> bool {
        match self {
            GoalSummary::Root(r) => r.is_completed,
            GoalSummary::Child(c) => c.is_completed,
        }
    }

    pub fn owner(&self) -> Option<&Owner> {
        match self {
            GoalSummary::Root(r) => r.owner.as_ref(),
            GoalSummary::Child(c) => c.owner.as_ref(),
        }
    }
}

// ---------------------------------------------------------------------------
// GoalNode – one node in the tree
// ---------------------------------------------------------------------------

pub struct GoalNode {
    pub summary: GoalSummary,
    pub detail: Option<Timestamped<Goal>>,
    pub children: Option<Timestamped<Vec<GoalNode>>>,
    pub comments: Option<Timestamped<Vec<Comment>>>,
    pub deliverables: Option<Timestamped<Vec<Deliverable>>>,
    pub expanded: bool,
}

// ---------------------------------------------------------------------------
// GoalTree – the full tree + cursor state
// ---------------------------------------------------------------------------

pub struct GoalTree {
    pub root: Timestamped<GoalNode>,
    pub cursor: usize,
    pub scroll_offset: usize,
}

// ---------------------------------------------------------------------------
// TreeRow – one flattened row for rendering
// ---------------------------------------------------------------------------

pub enum TreeRow<'a> {
    Goal {
        node: &'a GoalNode,
        depth: usize,
    },
    Detail {
        goal: &'a Goal,
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
}

// ---------------------------------------------------------------------------
// Tree operations
// ---------------------------------------------------------------------------

impl GoalTree {
    pub fn flatten(&self) -> Vec<TreeRow<'_>> {
        let mut rows = Vec::new();
        Self::flatten_node(&self.root.data, 0, &mut rows);

        rows
    }

    fn flatten_node<'a>(node: &'a GoalNode, depth: usize, rows: &mut Vec<TreeRow<'a>>) {
        rows.push(TreeRow::Goal { node, depth });

        if !node.expanded {
            return;
        }

        let child_depth = depth + 1;

        // Detail row (if fetched)
        if let Some(ref ts) = node.detail {
            rows.push(TreeRow::Detail {
                goal: &ts.data,
                depth: child_depth,
            });
        }

        // Comments section (if fetched)
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

        // Deliverables section (if fetched)
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

        // Child goal nodes (if fetched)
        if let Some(ref ts) = node.children {
            for child in &ts.data {
                Self::flatten_node(child, depth + 1, rows);
            }
        }
    }

    pub fn toggle_expand(&mut self) {
        let rows = self.flatten();
        if let Some(TreeRow::Goal { .. }) = rows.get(self.cursor) {
            // We need to find and mutate the corresponding node
            let mut idx = 0;
            Self::toggle_at(&mut self.root.data, self.cursor, &mut idx);
        }
    }

    fn toggle_at(node: &mut GoalNode, target: usize, idx: &mut usize) -> bool {
        if *idx == target {
            node.expanded = !node.expanded;
            return true;
        }
        *idx += 1;

        if node.expanded {
            // Skip detail row
            if node.detail.is_some() {
                *idx += 1;
            }
            // Skip comments
            if let Some(ts) = &node.comments {
                *idx += 1 + ts.data.len(); // header + items
            }
            // Skip deliverables
            if let Some(ts) = &node.deliverables {
                *idx += 1 + ts.data.len(); // header + items
            }
            // Recurse into children
            if let Some(ts) = &mut node.children {
                for t in &mut ts.data {
                    if Self::toggle_at(t, target, idx) {
                        return true;
                    }
                }
            }
        }

        false
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
            Some(TreeRow::Goal { node, .. }) if node.expanded => {
                // Collapse
                self.toggle_expand();
            }
            _ => {
                // Jump to parent Goal row: scan backwards for a Goal row at lesser depth
                let current_depth = rows.get(self.cursor).map(|r| r.depth()).unwrap_or(0);
                for i in (0..self.cursor).rev() {
                    if let TreeRow::Goal { depth, .. } = &rows[i]
                        && *depth < current_depth
                    {
                        self.cursor = i;
                        return;
                    }
                }
                // If at root depth, try collapsing if it's a Goal node
                if let Some(TreeRow::Goal { .. }) = rows.get(self.cursor) {
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
// Mock data builder
// ---------------------------------------------------------------------------

impl GoalTree {
    pub fn mock() -> Self {
        let root = GoalNode {
            summary: GoalSummary::Root(GoalTreeItem {
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
            }),
            detail: Some(Timestamped::now(Goal {
                id: "goal-001".into(),
                title: "Company Strategy".into(),
                description: Some("Overall company strategy for 2025".into()),
                body: None,
                status: Some(GoalStatus::Active),
                is_completed: false,
                completed_at: None,
                parent_id: None,
                organization_id: Some("org-001".into()),
                due_date: Some("2025-12-31".into()),
                created_at: Some("2025-01-01".into()),
                updated_at: Some("2025-04-01".into()),
                owner: Some(Owner {
                    id: "user-001".into(),
                    name: "Alice".into(),
                }),
            })),
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
                // Child: Q1 Goals – collapsed, no detail fetched
                GoalNode {
                    summary: GoalSummary::Child(GoalChildItem {
                        id: "goal-002".into(),
                        title: "Q1 Goals".into(),
                        description: None,
                        parent_id: Some("goal-001".into()),
                        status: Some(GoalStatus::Completed),
                        is_completed: true,
                        has_children: true,
                        order_no: 1.0,
                        owner: Some(Owner {
                            id: "user-001".into(),
                            name: "Alice".into(),
                        }),
                    }),
                    detail: None,
                    children: None,
                    comments: None,
                    deliverables: None,
                    expanded: false,
                },
                // Child: Q2 Goals – expanded with children
                GoalNode {
                    summary: GoalSummary::Child(GoalChildItem {
                        id: "goal-003".into(),
                        title: "Q2 Goals".into(),
                        description: Some("Q2 objectives".into()),
                        parent_id: Some("goal-001".into()),
                        status: Some(GoalStatus::InProgress),
                        is_completed: false,
                        has_children: true,
                        order_no: 2.0,
                        owner: Some(Owner {
                            id: "user-002".into(),
                            name: "Bob".into(),
                        }),
                    }),
                    detail: None,
                    children: Some(Timestamped::now(vec![
                        GoalNode {
                            summary: GoalSummary::Child(GoalChildItem {
                                id: "goal-004".into(),
                                title: "Revenue Target".into(),
                                description: None,
                                parent_id: Some("goal-003".into()),
                                status: Some(GoalStatus::InProgress),
                                is_completed: false,
                                has_children: false,
                                order_no: 1.0,
                                owner: Some(Owner {
                                    id: "user-003".into(),
                                    name: "Charlie".into(),
                                }),
                            }),
                            detail: None,
                            children: Some(Timestamped::now(vec![
                                GoalNode {
                                    summary: GoalSummary::Child(GoalChildItem {
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
                                    }),
                                    detail: None,
                                    children: None,
                                    comments: None,
                                    deliverables: None,
                                    expanded: false,
                                },
                                GoalNode {
                                    summary: GoalSummary::Child(GoalChildItem {
                                        id: "goal-007".into(),
                                        parent_id: None,
                                        title: "Infrastructure Upgrade".into(),
                                        description: None,
                                        status: Some(GoalStatus::InProgress),
                                        order_no: 3.0,
                                        is_completed: false,
                                        has_children: false,
                                        owner: Some(Owner {
                                            id: "user-003".into(),
                                            name: "Charlie".into(),
                                        }),
                                    }),
                                    detail: None,
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
                        GoalNode {
                            summary: GoalSummary::Child(GoalChildItem {
                                id: "goal-005".into(),
                                title: "Customer Acquisition".into(),
                                description: None,
                                parent_id: Some("goal-003".into()),
                                status: Some(GoalStatus::Active),
                                is_completed: false,
                                has_children: false,
                                order_no: 2.0,
                                owner: Some(Owner {
                                    id: "user-004".into(),
                                    name: "Diana".into(),
                                }),
                            }),
                            detail: None,
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
