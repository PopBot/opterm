//! Bounded, window-local navigation and view recovery. Never stores launch commands.
use opterm::layouts::{Node, SavedTab, SessionRef};
use std::collections::VecDeque;

pub const RECENT_CLOSE_LIMIT: usize = 20;
const RECENT_USE_LIMIT: usize = 128;

pub struct ClosedTab {
    pub tab: SavedTab,
    pub key: u64,
    pub index: usize,
    pub closed: Vec<SessionRef>,
    pub survivors: Vec<SessionRef>,
}

pub struct RestorePlan {
    pub tab: SavedTab,
    pub replace: Option<u64>,
}

impl ClosedTab {
    /// Only replace a surviving tab when its membership still matches. Otherwise
    /// restore the closed branch separately, preserving the user's later edits.
    pub fn plan(&self, displayed: &[(u64, Vec<SessionRef>)]) -> Option<RestorePlan> {
        let already_visible: Vec<_> = displayed.iter().flat_map(|(_, ids)| ids).collect();
        let closed: Vec<_> = self
            .closed
            .iter()
            .filter(|session| !already_visible.contains(session))
            .cloned()
            .collect();
        if closed.is_empty() {
            return None;
        }
        let replace = (!self.survivors.is_empty())
            .then(|| displayed.iter().find(|(key, _)| *key == self.key))
            .flatten()
            .filter(|(_, ids)| {
                ids.len() == self.survivors.len()
                    && ids.iter().all(|id| self.survivors.contains(id))
                    && closed.len() == self.closed.len()
            })
            .map(|(key, _)| *key);
        let members = if replace.is_some() {
            self.tab.layout.sessions()
        } else {
            closed
        };
        let layout = retain_node(&self.tab.layout, &members)?;
        let focused = self.tab.layout.sessions().get(self.tab.focused).cloned();
        let focused = layout
            .sessions()
            .iter()
            .position(|session| Some(session) == focused.as_ref())
            .unwrap_or(0);
        Some(RestorePlan {
            tab: SavedTab {
                layout,
                focused,
                name: self.tab.name.clone(),
                maximized: self.tab.maximized,
            },
            replace,
        })
    }
}

fn retain_node(node: &Node, members: &[SessionRef]) -> Option<Node> {
    match node {
        Node::Pane { session, .. } => members.contains(session).then(|| node.clone()),
        Node::Split {
            axis,
            ratio,
            first,
            second,
        } => match (retain_node(first, members), retain_node(second, members)) {
            (Some(first), Some(second)) => Some(Node::Split {
                axis: *axis,
                ratio: *ratio,
                first: Box::new(first),
                second: Box::new(second),
            }),
            (first, second) => first.or(second),
        },
    }
}

#[derive(Default)]
pub struct RecentlyUsed(VecDeque<SessionRef>);
impl RecentlyUsed {
    pub fn visit(&mut self, session: SessionRef) {
        self.0.retain(|other| other != &session);
        self.0.push_front(session);
        self.0.truncate(RECENT_USE_LIMIT);
    }
    pub fn rank(&self, session: &SessionRef) -> usize {
        self.0
            .iter()
            .position(|other| other == session)
            .unwrap_or(RECENT_USE_LIMIT)
    }
}

pub struct Candidate {
    pub session: SessionRef,
    pub title: String,
    pub detail: String,
    pub search: String,
}
impl Candidate {
    pub fn matches(&self, query: &str) -> bool {
        let search = self.search.to_lowercase();
        query
            .split_whitespace()
            .all(|word| search.contains(&word.to_lowercase()))
    }
}

#[derive(Default)]
pub struct Switcher {
    pub query: String,
    pub selected: usize,
    pub focus_query: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use opterm::layout::Axis;

    fn reference(id: u64) -> SessionRef {
        SessionRef {
            instance: "service-a".into(),
            id,
        }
    }
    fn pane(id: u64) -> Node {
        Node::Pane {
            session: reference(id),
            title: format!("Shell {id}"),
        }
    }
    fn closed() -> ClosedTab {
        ClosedTab {
            tab: SavedTab {
                name: Some("Build".into()),
                focused: 1,
                maximized: true,
                layout: Node::Split {
                    axis: Axis::Right,
                    ratio: 0.37,
                    first: Box::new(pane(1)),
                    second: Box::new(pane(2)),
                },
            },
            key: 1,
            index: 3,
            closed: vec![reference(1), reference(2)],
            survivors: vec![],
        }
    }
    #[test]
    fn whole_tab_retains_focus_name_and_split_geometry() {
        let closed = closed();
        let plan = closed.plan(&[]).unwrap();
        assert_eq!(plan.tab, closed.tab);
        assert_eq!(plan.replace, None);
    }
    #[test]
    fn pane_undo_rejoins_unchanged_survivors_without_duplicates() {
        let mut closed = closed();
        closed.closed = vec![reference(2)];
        closed.survivors = vec![reference(1)];
        let plan = closed.plan(&[(1, vec![reference(1)])]).unwrap();
        assert_eq!(plan.tab, closed.tab);
        assert_eq!(plan.replace, Some(1));
        let changed = closed
            .plan(&[(1, vec![reference(1), reference(3)])])
            .unwrap();
        assert_eq!(changed.tab.layout.sessions(), vec![reference(2)]);
        assert_eq!(changed.replace, None);
        assert!(closed.plan(&[(7, vec![reference(2)])]).is_none());
    }
    #[test]
    fn restored_branch_omits_sessions_reopened_elsewhere() {
        let plan = closed().plan(&[(9, vec![reference(2)])]).unwrap();
        assert_eq!(plan.tab.layout.sessions(), vec![reference(1)]);
        assert_eq!(plan.tab.focused, 0);
    }
    #[test]
    fn mru_is_bounded_and_service_identity_does_not_collide() {
        let mut mru = RecentlyUsed::default();
        for id in 0..256 {
            mru.visit(reference(id));
        }
        assert_eq!(mru.0.len(), RECENT_USE_LIMIT);
        mru.visit(reference(200));
        assert_eq!(mru.rank(&reference(200)), 0);
        assert_ne!(
            mru.rank(&SessionRef {
                instance: "service-b".into(),
                id: 200
            }),
            0
        );
        let candidate = Candidate {
            session: reference(1),
            title: "Build".into(),
            detail: "".into(),
            search: "Build release · shell profile · Other window".into(),
        };
        assert!(candidate.matches("SHELL build"));
        assert!(!candidate.matches("shell debug"));
    }
}
