use crate::{vertex::VertexPair, wire::WireFormat, Vertex, VertexHash};
use std::{collections::HashMap, sync::Arc};

/// A [`ConflictGraph`] maps every [`Vertex`] to the an ordering [`Constraint`] and the set of
/// vertices which agree or disagree on that constraint.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct ConflictGraph {
    graph: HashMap<VertexPair, Constraint>,
}

impl ConflictGraph {
    /// Construct a new [`ConflictGraph`].
    pub fn new() -> ConflictGraph {
        ConflictGraph {
            graph: HashMap::new(),
        }
    }

    /// Insert a [`Vertex`] into the [`ConflictGraph`], mapping its parents to their ordering
    /// [`Constraint`]s, and registering this [`Vertex`]'s preference on each [`Constraint`].
    pub fn insert(&mut self, vx: &Arc<Vertex>) {
        for pair in vx.parent_pairs() {
            if let Some(constraint) = self.graph.get_mut(&pair) {
                constraint.track(vx);
            } else {
                let mut constraint = Constraint::new(pair.clone());
                constraint.track(vx);
                self.graph.insert(pair, constraint);
            }
        }
    }
}

/// A [`Constraint`] contains a [`Vertex`] pair, and the set of vertices which agree or disagree
/// with a particular ordering constraing for that vertex pair.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
struct Constraint {
    pair: VertexPair,
    left_first: HashMap<VertexHash, Arc<Vertex>>,
    right_first: HashMap<VertexHash, Arc<Vertex>>,
}

impl Constraint {
    /// Construct a new [`Constraint`] from a [`VertexPair`].
    fn new(pair: VertexPair) -> Constraint {
        Constraint {
            pair,
            left_first: HashMap::new(),
            right_first: HashMap::new(),
        }
    }

    /// Track a [`Vertex`] if it has a preference on the constraint
    fn track(&mut self, vx: &Arc<Vertex>) {
        let pos_left = vx
            .parents
            .iter()
            .position(|h| h == &self.pair.0)
            .expect("left not found in parents");
        let pos_right = vx
            .parents
            .iter()
            .position(|h| h == &self.pair.1)
            .expect("right not found in parents");
        if pos_left < pos_right {
            self.left_first.insert(vx.hash(), vx.clone());
        } else {
            self.right_first.insert(vx.hash(), vx.clone());
        }
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn new_graph() {
        todo!()
    }

    #[test]
    fn graph_insert() {
        todo!()
    }

    #[test]
    fn new_constraint() {
        todo!()
    }

    #[test]
    fn constraint_track() {
        todo!()
    }

    #[test]
    fn new_pair() {
        todo!()
    }
}
