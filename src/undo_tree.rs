//! Undo as a tree rather than a list. Undoing and then typing starts a new branch, and what was undone stays there.
//!
//! Each node is a saved state of the document. The text being edited either is the state at `at`, or has moved on
//! from it (`dirty`). Undo and redo walk along one line through the tree, so they behave as they always have.

use std::collections::{HashMap, HashSet};

pub type NodeId = u64;

pub struct Node<S> {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    /// The child that redo goes to: whichever was last visited.
    pub preferred: Option<NodeId>,
    pub state: S,
    /// Seconds since 1970.
    pub when: u64,
    pub label: String,
}

pub struct UndoTree<S> {
    /// In order of id, which is also the order they were made.
    nodes: Vec<Node<S>>,
    at: Option<NodeId>,
    dirty: bool,
    next_id: NodeId,
}

impl<S> Default for UndoTree<S> {
    fn default() -> Self {
        Self { nodes: Vec::new(), at: None, dirty: false, next_id: 1 }
    }
}

/// One row of the picture of the history, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// None is the text as it is now, where that isn't a saved state yet.
    pub id: Option<NodeId>,
    /// Which column of the picture it's in.
    pub lane: usize,
    /// The row of what it came from.
    pub parent_row: Option<usize>,
    pub when: u64,
    pub label: String,
    /// It's where the text is now.
    pub here: bool,
    /// It's on the line that undo would walk back along.
    pub on_path: bool,
}

pub fn now_seconds() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

impl<S> UndoTree<S> {
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.at = None;
        self.dirty = false;
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn at(&self) -> Option<NodeId> {
        self.at
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn next_id(&self) -> NodeId {
        self.next_id
    }

    pub fn nodes(&self) -> &[Node<S>] {
        &self.nodes
    }

    fn index(&self, id: NodeId) -> Option<usize> {
        self.nodes.binary_search_by_key(&id, |node| node.id).ok()
    }

    pub fn node(&self, id: NodeId) -> Option<&Node<S>> {
        self.index(id).map(|index| &self.nodes[index])
    }

    fn node_mut(&mut self, id: NodeId) -> Option<&mut Node<S>> {
        self.index(id).map(|index| &mut self.nodes[index])
    }

    fn parent_of(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).and_then(|node| node.parent)
    }

    pub fn can_undo(&self) -> bool {
        self.dirty || self.at.is_some_and(|at| self.parent_of(at).is_some())
    }

    pub fn can_redo(&self) -> bool {
        !self.dirty && self.at.and_then(|at| self.node(at)).is_some_and(|node| node.preferred.is_some())
    }

    /// The text is exactly the state at a node, so that node can serve as the state before an edit.
    pub fn is_clean_here(&self) -> bool {
        self.at.is_some() && !self.dirty
    }

    pub fn set_dirty(&mut self, dirty: bool) {
        self.dirty = dirty && self.at.is_some();
    }

    /// Replaces the state here, for details that change without an undo step, such as where the cursor is.
    pub fn refresh_here(&mut self, state: S) {
        if let Some(at) = self.at
            && let Some(node) = self.node_mut(at)
        {
            node.state = state;
        }
    }

    /// Adds a state after the one here, and moves on from it.
    pub fn push(&mut self, state: S, label: String) -> NodeId {
        self.push_at(state, label, now_seconds())
    }

    fn push_at(&mut self, state: S, label: String, when: u64) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        if let Some(at) = self.at
            && let Some(parent) = self.node_mut(at)
        {
            parent.preferred = Some(id);
        }
        self.nodes.push(Node { id, parent: self.at, preferred: None, state, when, label });
        self.at = Some(id);
        self.dirty = true;
        id
    }

    /// Before going elsewhere: what's being left is kept, as a new state if it has moved on, so nothing is lost.
    pub fn leave(&mut self, state: S, label: String) {
        if self.dirty {
            self.push(state, label);
            self.dirty = false;
        } else {
            self.refresh_here(state);
        }
    }

    /// One step back along the line. Call `leave` first.
    pub fn undo(&mut self) -> Option<&S> {
        let parent = self.parent_of(self.at?)?;
        self.at = Some(parent);
        self.node(parent).map(|node| &node.state)
    }

    /// One step forward, along the branch last visited. Call `leave` first.
    pub fn redo(&mut self) -> Option<&S> {
        let next = self.node(self.at?)?.preferred?;
        self.at = Some(next);
        self.node(next).map(|node| &node.state)
    }

    /// Goes straight to any state. Redo and undo then follow the line through it. Call `leave` first.
    pub fn jump(&mut self, target: NodeId) -> Option<&S> {
        self.index(target)?;
        let mut child = target;
        while let Some(parent) = self.parent_of(child) {
            if let Some(node) = self.node_mut(parent) {
                node.preferred = Some(child);
            }
            child = parent;
        }
        self.at = Some(target);
        self.dirty = false;
        self.node(target).map(|node| &node.state)
    }

    /// Lets go of everything made from `first_new` on, except `base`, which is then where the text moved on from.
    /// It's how everything a script did becomes one step.
    pub fn squash_to(&mut self, base: NodeId, first_new: NodeId) {
        self.nodes.retain(|node| node.id < first_new || node.id == base);
        self.at = Some(base);
        let known: HashSet<NodeId> = self.nodes.iter().map(|node| node.id).collect();
        let mut latest_child: HashMap<NodeId, NodeId> = HashMap::new();
        for node in &self.nodes {
            if let Some(parent) = node.parent {
                latest_child.insert(parent, node.id);
            }
        }
        for node in &mut self.nodes {
            if node.preferred.is_some_and(|id| !known.contains(&id)) {
                node.preferred = latest_child.get(&node.id).copied();
            }
        }
    }

    fn path_to_here(&self) -> HashSet<NodeId> {
        let mut path = HashSet::new();
        let mut step = self.at;
        while let Some(id) = step {
            path.insert(id);
            step = self.parent_of(id);
        }
        path
    }

    /// Takes a state out, joining what came from it to what it came from.
    fn remove(&mut self, id: NodeId) {
        let Some(index) = self.index(id) else {
            return;
        };
        let removed = self.nodes.remove(index);
        let mut last_child = None;
        for node in &mut self.nodes {
            if node.parent == Some(id) {
                node.parent = removed.parent;
                last_child = Some(node.id);
            }
        }
        if let Some(parent) = removed.parent {
            let latest = self.nodes.iter().rev().find(|node| node.parent == Some(parent)).map(|node| node.id);
            if let Some(node) = self.node_mut(parent)
                && node.preferred == Some(id)
            {
                node.preferred = last_child.or(latest);
            }
        }
    }

    /// Lets go of the oldest states until there are no more than `limit`. The line back from here goes last. A script
    /// running gives `group`: its first state, which must stay, and where its own begin, which go before anything older.
    pub fn prune(&mut self, limit: usize, group: Option<(NodeId, NodeId)>) {
        while self.nodes.len() > limit {
            let mut victim = None;
            if let Some((base, first_new)) = group {
                victim = self.nodes.iter().find(|node| node.id >= first_new && node.id != base && Some(node.id) != self.at).map(|node| node.id);
            }
            if victim.is_none() {
                let path = self.path_to_here();
                let keep = group.map(|(base, _)| base);
                let has_children: HashMap<NodeId, usize> = self.nodes.iter().filter_map(|node| node.parent).fold(HashMap::new(), |mut counts, parent| {
                    *counts.entry(parent).or_insert(0) += 1;
                    counts
                });
                victim = self
                    .nodes
                    .iter()
                    .find(|node| {
                        let children = has_children.get(&node.id).copied().unwrap_or(0);
                        let spare_leaf = children == 0 && !path.contains(&node.id);
                        let spare_root = node.parent.is_none() && children == 1 && Some(node.id) != self.at;
                        (spare_leaf || spare_root) && Some(node.id) != keep
                    })
                    .map(|node| node.id);
            }
            match victim {
                Some(id) => self.remove(id),
                None => return,
            }
        }
    }

    /// For the picture: which column each state goes in. A state continues its parent's column, unless a sibling got there first.
    pub fn rows(&self, now_label: &str) -> Vec<Row> {
        let path = self.path_to_here();
        let mut lanes: HashMap<NodeId, usize> = HashMap::new();
        let mut continued: HashSet<NodeId> = HashSet::new();
        let mut next_lane = 0;
        let mut new_lane = || {
            let lane = next_lane;
            next_lane += 1;
            lane
        };
        for node in &self.nodes {
            let lane = match node.parent {
                Some(parent) if continued.insert(parent) => lanes.get(&parent).copied().unwrap_or(0),
                _ => new_lane(),
            };
            lanes.insert(node.id, lane);
        }
        let mut rows = Vec::with_capacity(self.nodes.len() + 1);
        if self.dirty
            && let Some(at) = self.at
        {
            let lane = if continued.contains(&at) { new_lane() } else { lanes.get(&at).copied().unwrap_or(0) };
            rows.push(Row { id: None, lane, parent_row: None, when: now_seconds(), label: now_label.to_string(), here: true, on_path: true });
        }
        let offset = rows.len();
        let row_of: HashMap<NodeId, usize> = self.nodes.iter().rev().enumerate().map(|(row, node)| (node.id, row + offset)).collect();
        if self.dirty
            && let (Some(first), Some(at)) = (rows.first_mut(), self.at)
        {
            first.parent_row = row_of.get(&at).copied();
        }
        for node in self.nodes.iter().rev() {
            rows.push(Row {
                id: Some(node.id),
                lane: lanes.get(&node.id).copied().unwrap_or(0),
                parent_row: node.parent.and_then(|parent| row_of.get(&parent).copied()),
                when: node.when,
                label: node.label.clone(),
                here: !self.dirty && self.at == Some(node.id),
                on_path: path.contains(&node.id),
            });
        }
        rows
    }

    /// Puts together a tree read back from a file: each one's parent comes before it, and the text is at `current`.
    pub fn rebuilt(nodes: Vec<(Option<usize>, u64, String, S)>, current: usize) -> Self {
        let mut tree = Self::default();
        let mut ids = Vec::with_capacity(nodes.len());
        for (parent, when, label, state) in nodes {
            tree.at = parent.and_then(|index| ids.get(index).copied());
            ids.push(tree.push_at(state, label, when));
        }
        tree.at = ids.get(current).copied();
        tree.dirty = false;
        if let Some(target) = tree.at {
            tree.jump(target);
        }
        tree
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A document that's just a word, edited as a person would.
    struct Editor {
        text: String,
        tree: UndoTree<String>,
    }

    impl Editor {
        fn new(text: &str) -> Self {
            Self { text: text.to_string(), tree: UndoTree::default() }
        }

        fn edit(&mut self, text: &str) {
            if self.tree.is_clean_here() {
                self.tree.refresh_here(self.text.clone());
                self.tree.set_dirty(true);
            } else {
                self.tree.push(self.text.clone(), self.text.clone());
            }
            self.text = text.to_string();
        }

        fn undo(&mut self) {
            if self.tree.can_undo() {
                self.tree.leave(self.text.clone(), self.text.clone());
                self.text = self.tree.undo().unwrap().clone();
            }
        }

        fn redo(&mut self) {
            if self.tree.can_redo() {
                self.tree.leave(self.text.clone(), self.text.clone());
                self.text = self.tree.redo().unwrap().clone();
            }
        }

        fn jump(&mut self, id: NodeId) {
            self.tree.leave(self.text.clone(), self.text.clone());
            self.text = self.tree.jump(id).unwrap().clone();
        }

        fn id_of(&self, state: &str) -> NodeId {
            self.tree.nodes().iter().find(|node| node.state == state).unwrap().id
        }
    }

    #[test]
    fn undo_and_redo_behave_as_a_plain_list_would() {
        let mut e = Editor::new("a");
        assert!(!e.tree.can_undo() && !e.tree.can_redo());
        e.edit("b");
        e.edit("c");
        assert!(e.tree.can_undo() && !e.tree.can_redo());
        e.undo();
        assert_eq!(e.text, "b");
        e.undo();
        assert_eq!(e.text, "a");
        assert!(!e.tree.can_undo());
        e.redo();
        assert_eq!(e.text, "b");
        e.redo();
        assert_eq!(e.text, "c");
        assert!(!e.tree.can_redo());
        e.undo();
        assert_eq!(e.text, "b");
    }

    #[test]
    fn typing_after_undo_keeps_what_was_undone_on_its_own_branch() {
        let mut e = Editor::new("a");
        e.edit("b");
        e.edit("old branch");
        e.undo();
        e.edit("new branch");
        assert!(!e.tree.can_redo(), "redo has nothing ahead on this line");
        e.undo();
        assert_eq!(e.text, "b");
        e.redo();
        assert_eq!(e.text, "new branch", "redo follows the branch last used");
        e.jump(e.id_of("old branch"));
        assert_eq!(e.text, "old branch");
        e.undo();
        assert_eq!(e.text, "b");
        e.redo();
        assert_eq!(e.text, "old branch", "and now it follows this one");
    }

    #[test]
    fn going_elsewhere_keeps_what_was_being_written() {
        let mut e = Editor::new("a");
        e.edit("b");
        e.edit("c");
        e.jump(e.id_of("a"));
        assert_eq!(e.text, "a");
        assert!(e.tree.nodes().iter().any(|node| node.state == "c"), "the text being left is a state now");
        e.jump(e.id_of("c"));
        assert_eq!(e.text, "c");
    }

    #[test]
    fn the_picture_shows_branches_in_their_own_columns() {
        let mut e = Editor::new("a");
        e.edit("b");
        e.edit("first");
        e.undo();
        e.edit("second");
        let rows = e.tree.rows("Now");
        assert_eq!(rows[0].id, None, "the text as it is goes on top");
        assert!(rows[0].here && rows[0].on_path);
        let row_of = |state: &str| rows.iter().position(|row| row.id == Some(e.id_of(state))).unwrap();
        let (b, first) = (row_of("b"), row_of("first"));
        assert_eq!(rows[first].parent_row, Some(b));
        assert_eq!(rows[0].parent_row, Some(b));
        assert_ne!(rows[0].lane, rows[first].lane, "the two branches don't share a column");
        assert_eq!(rows[first].lane, rows[b].lane, "the first branch carries on down its parent's");
        assert!(!rows[first].on_path, "undo wouldn't go through the other branch");
        assert!(rows[b].on_path);
        // Clean, the state itself is marked as where the text is.
        e.jump(e.id_of("first"));
        let rows = e.tree.rows("Now");
        assert!(rows.iter().all(|row| row.id.is_some()));
        assert_eq!(rows.iter().filter(|row| row.here).count(), 1);
    }

    #[test]
    fn the_oldest_go_first_with_the_way_back_kept_longest() {
        let mut e = Editor::new("0");
        for n in 1..10 {
            e.edit(&n.to_string());
        }
        e.jump(e.id_of("4"));
        e.edit("branch");
        e.jump(e.id_of("9"));
        e.tree.prune(6, None);
        assert_eq!(e.tree.len(), 6);
        let kept: Vec<&str> = e.tree.nodes().iter().map(|node| node.state.as_str()).collect();
        assert!(kept.contains(&"9") && kept.contains(&"8"), "the way back from here: {kept:?}");
        assert!(!kept.contains(&"0"), "the oldest went first: {kept:?}");
        // What's left is still one connected tree, and undo walks down it.
        let mut steps = 0;
        while e.tree.can_undo() {
            e.undo();
            steps += 1;
        }
        assert!(steps > 0);
        assert_eq!(e.tree.nodes().iter().filter(|node| node.parent.is_none()).count(), 1);
    }

    #[test]
    fn pruning_a_tree_with_nothing_spare_stops_rather_than_breaking_it() {
        let mut e = Editor::new("a");
        e.edit("b");
        e.edit("c");
        e.tree.prune(0, None);
        assert_eq!(e.tree.len(), 1, "the state here stays");
        assert!(e.tree.at().is_some());
    }

    #[test]
    fn a_scripts_steps_can_become_one() {
        let mut e = Editor::new("start");
        e.edit("before");
        let first_new = e.tree.next_id();
        e.edit("one");
        let base = e.tree.at().unwrap();
        e.edit("two");
        e.edit("three");
        e.tree.prune(3, Some((base, first_new)));
        assert!(e.tree.node(base).is_some(), "the state before the script began stays");
        e.tree.squash_to(base, first_new);
        e.tree.set_dirty(true);
        e.undo();
        assert_eq!(e.text, "before");
        e.undo();
        assert_eq!(e.text, "start");
        e.redo();
        e.redo();
        assert_eq!(e.text, "three", "redo isn't left pointing at something gone");
    }

    #[test]
    fn a_tree_can_be_rebuilt_from_what_a_file_held() {
        let tree: UndoTree<String> = UndoTree::rebuilt(vec![(None, 5, "r".into(), "root".into()), (Some(0), 6, "a".into(), "a".into()), (Some(0), 7, "b".into(), "b".into())], 1);
        assert!(!tree.is_dirty());
        assert_eq!(tree.at(), Some(tree.nodes()[1].id));
        assert!(tree.can_undo());
        assert!(!tree.can_redo());
        assert_eq!(tree.nodes()[2].parent, Some(tree.nodes()[0].id));
        assert_eq!(tree.nodes()[2].when, 7);
        assert_eq!(tree.node(tree.nodes()[0].id).unwrap().preferred, Some(tree.nodes()[1].id), "redo from the root comes back here");
    }
}
