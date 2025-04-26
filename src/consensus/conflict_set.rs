use crate::{wire::WireFormat, Vertex, VertexHash};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct ConflictSet {
    pub owner: Arc<Vertex>,
    pub preferred: bool,
    parents: HashSet<VertexHash>,
    conflicts: HashMap<VertexHash, Arc<Vertex>>,
}

impl ConflictSet {
    /// Create a new instance of a [`ConflictSet`]
    pub fn new(vx: &Arc<Vertex>) -> ConflictSet {
        let parents: HashSet<_> = vx.parents.iter().copied().collect();
        debug_assert_eq!(vx.parents.len(), parents.len(), "parents must not repeat");
        ConflictSet {
            owner: vx.clone(),
            preferred: false,
            parents,
            conflicts: HashMap::new(),
        }
    }

    /// Returns true if the given [`Vertex`] conflicts with the [`ConflictSet`] owner
    fn conflicts_with(&self, vx: &Vertex) -> bool {
        let rh_set: HashSet<_> = vx.parents.iter().copied().collect();
        let intersection: HashSet<_> = self.parents.intersection(&rh_set).collect();
        let lh_ordered: Vec<_> = self
            .owner
            .parents
            .iter()
            .filter(|p| intersection.contains(p))
            .collect();
        let rh_ordered: Vec<_> = vx
            .parents
            .iter()
            .filter(|p| intersection.contains(p))
            .collect();
        lh_ordered != rh_ordered
    }

    /// Check if the given vertex conflicts with this vertex, and if so, adds it to the set.
    /// Returns true if conflict.
    pub fn add_if_conflict(&mut self, vx: &Arc<Vertex>) -> bool {
        if self.conflicts_with(vx) {
            self.add(vx);
            true
        } else {
            false
        }
    }

    /// Add the given [`Vertex`] as a conflict to track, without confirming they conflict. This
    /// should really only be used if a conflict is certain, and you don't want to perform the
    /// expensive conflict checks again.
    pub fn add(&mut self, vx: &Arc<Vertex>) {
        self.conflicts.insert(vx.hash(), vx.clone());
    }
}

#[cfg(test)]
mod test {
    use super::ConflictSet;
    use crate::Vertex;
    use std::sync::Arc;

    #[test]
    fn new() {
        let vx = Arc::new(Vertex::default());
        let cs = ConflictSet::new(&vx);
        assert_eq!(cs.owner, vx);
        assert_eq!(cs.preferred, false);
        assert!(cs.conflicts.is_empty());
    }

    #[test]
    fn conflicts_with() {
        todo!();
    }

    #[test]
    fn add_if_conflict() {
        todo!();
    }

    #[test]
    fn add() {
        todo!();
    }
}
