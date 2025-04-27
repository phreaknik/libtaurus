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

    /// Return an iterator which yields each [`Vertex`] which conflicts with the specified
    pub fn conflicts_of(&self, vx: &Arc<Vertex>) -> HashMap<&VertexHash, &Arc<Vertex>> {
        vx.parent_pairs()
            .map(|pair| self.graph[&pair].conflicts_of(vx).iter())
            .flatten()
            .collect()
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

    /// Returns true if the given [`Vertex`] observed the left [`Vertex`] in the constraint pair
    /// first, before the right [`Vertex`].
    fn chooses_left_first(&self, vx: &Arc<Vertex>) -> bool {
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
        pos_left < pos_right
    }

    /// Track a [`Vertex`] if it has a preference on the constraint
    fn track(&mut self, vx: &Arc<Vertex>) {
        if self.chooses_left_first(vx) {
            self.left_first.insert(vx.hash(), vx.clone());
        } else {
            self.right_first.insert(vx.hash(), vx.clone());
        }
    }

    /// Return an iterator which yields each [`Vertex`] which conflicts with the specified
    fn conflicts_of(&self, vx: &Arc<Vertex>) -> &HashMap<VertexHash, Arc<Vertex>> {
        if self.chooses_left_first(vx) {
            &self.right_first
        } else {
            &self.left_first
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{
        consensus::{conflict_set::ConflictGraph, namespace::Namespace},
        vertex::VertexPair,
        Vertex, WireFormat,
    };
    use itertools::Itertools;
    use rand::{distributions::Alphanumeric, Rng};
    use std::{collections::HashMap, sync::Arc};

    use super::Constraint;

    // Helper to build test vertices with unique hashes
    pub fn test_vertex<'a, P>(parents: P) -> Arc<Vertex>
    where
        P: IntoIterator<Item = &'a Arc<Vertex>>,
    {
        let rand_namespace = Namespace::new(
            &rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(30)
                .map(char::from)
                .collect::<String>(),
        );
        Arc::new(
            Vertex::empty()
                .with_parents(parents)
                .unwrap()
                .in_namespace(rand_namespace)
                .unwrap(),
        )
    }

    #[test]
    fn new_graph() {
        let cflcts = ConflictGraph::new();
        assert_eq!(cflcts.graph, HashMap::new());
    }

    #[test]
    fn insert_and_check_conflicts() {
        // Construct test vertices with parent ordering conflicts
        let v11 = Arc::new(Vertex::empty());
        let v21 = test_vertex([&v11]);
        let v22 = test_vertex([&v11]);
        let v31 = test_vertex([&v21]);
        let v32 = test_vertex([&v21, &v22]);
        let v33 = test_vertex([&v22, &v21]); // Conflicts with v32
        let v41 = test_vertex([&v31, &v32, &v33]);
        let v42 = test_vertex([&v31, &v33]);
        let v43 = test_vertex([&v33, &v32, &v31]); // Conflicts with v41, v42
        let v44 = test_vertex([&v33, &v31]); // Conflicts with v41, v42
        let v45 = test_vertex([&v32, &v31]); // Conflicts with v41

        // Insert the vertices
        let mut cflcts = ConflictGraph::new();
        assert_eq!(cflcts.graph.len(), 0);
        cflcts.insert(&v11);
        cflcts.insert(&v21);
        cflcts.insert(&v22);
        cflcts.insert(&v31);
        cflcts.insert(&v32);
        cflcts.insert(&v33);
        cflcts.insert(&v41);
        cflcts.insert(&v42);
        cflcts.insert(&v43);
        cflcts.insert(&v43); // Double insertion should not change outcome
        cflcts.insert(&v44);
        cflcts.insert(&v45);

        // Check that each vertex has the expected conflicts
        let should_conflict = |dut, expected: &[&Arc<Vertex>]| {
            expected
                .iter()
                .map(|&vx| (vx.hash(), vx))
                .sorted_by_key(|e| e.0)
                .eq(cflcts
                    .conflicts_of(dut)
                    .into_iter()
                    .map(|(hash, vx)| (hash.clone(), vx))
                    .sorted_by_key(|e| e.0))
        };
        assert!(should_conflict(&v11, &[]));
        assert!(should_conflict(&v21, &[]));
        assert!(should_conflict(&v22, &[]));
        assert!(should_conflict(&v31, &[]));
        assert!(should_conflict(&v32, &[&v33]));
        assert!(should_conflict(&v33, &[&v32]));
        assert!(should_conflict(&v41, &[&v43, &v44, &v45]));
        assert!(should_conflict(&v42, &[&v43, &v44]));
        assert!(should_conflict(&v43, &[&v41, &v42]));
        assert!(should_conflict(&v44, &[&v41, &v42]));
        assert!(should_conflict(&v45, &[&v41]));
    }

    #[test]
    fn new_constraint() {
        let gen = Arc::new(Vertex::empty());
        let v1 = test_vertex([&gen]);
        let v2 = test_vertex([&gen]);
        let cx = Constraint::new(VertexPair::new(v1.hash(), v2.hash()));
        assert_eq!(cx.pair, VertexPair::new(v1.hash(), v2.hash()));
        assert!(cx.left_first.is_empty());
        assert!(cx.right_first.is_empty());
    }

    #[test]
    fn constraint_chooses_left_first() {
        let gen = Arc::new(Vertex::empty());
        let p1 = test_vertex([&gen]);
        let p2 = test_vertex([&gen]);
        let c1 = test_vertex([&p1, &p2]);
        let c2 = test_vertex([&p2, &p1]);
        let cx = Constraint::new(VertexPair::new(p1.hash(), p2.hash()));
        if p1.hash() < p2.hash() {
            assert!(cx.chooses_left_first(&c1));
            assert!(!cx.chooses_left_first(&c2));
        } else {
            assert!(!cx.chooses_left_first(&c1));
            assert!(cx.chooses_left_first(&c2));
        }
    }
    #[test]
    fn constraint_track() {
        let gen = Arc::new(Vertex::empty());
        let p1 = test_vertex([&gen]);
        let p2 = test_vertex([&gen]);
        let c1 = test_vertex([&p1, &p2]);
        let c2 = test_vertex([&p2, &p1]);
        let mut cx = Constraint::new(VertexPair::new(p1.hash(), p2.hash()));
        assert_eq!(cx.pair, VertexPair::new(p1.hash(), p2.hash()));
        assert!(cx.left_first.is_empty());
        assert!(cx.right_first.is_empty());
        cx.track(&c1);
        if cx.chooses_left_first(&c1) {
            assert_eq!(cx.left_first.len(), 1);
            assert!(cx.right_first.is_empty());
        } else {
            assert!(cx.left_first.is_empty());
            assert_eq!(cx.right_first.len(), 1);
        }
        cx.track(&c2);
        assert_eq!(cx.left_first.len(), 1);
        assert_eq!(cx.right_first.len(), 1);
        cx.track(&c1); // Duplicate insertion should have no effect
        assert_eq!(cx.left_first.len(), 1);
        assert_eq!(cx.right_first.len(), 1);
    }

    #[test]
    #[should_panic]
    fn try_track_unrelated() {
        let gen = Arc::new(Vertex::empty());
        let p1 = test_vertex([&gen]);
        let p2 = test_vertex([&gen]);
        let unrelated = test_vertex([]);
        let mut cx = Constraint::new(VertexPair::new(p1.hash(), p2.hash()));
        cx.track(&unrelated); // Should panic
    }

    #[test]
    fn constraint_conflicts_of() {
        let gen = Arc::new(Vertex::empty());
        let p1 = test_vertex([&gen]);
        let p2 = test_vertex([&gen]);
        let p3 = test_vertex([&gen]);
        let c1 = test_vertex([&p1, &p2]);
        let c2 = test_vertex([&p2, &p1]);
        let c3 = test_vertex([&p1, &p2, &p3]);
        let c4 = test_vertex([&p2, &p1, &p3]);
        let c5 = test_vertex([&p3, &p1, &p2]);
        let c6 = test_vertex([&p3, &p2, &p1]);
        let mut cx = Constraint::new(VertexPair::new(p1.hash(), p2.hash()));
        cx.track(&c1);
        cx.track(&c2);
        cx.track(&c3);
        cx.track(&c4);
        cx.track(&c5);
        cx.track(&c6);
        let should_conflict = |dut, expected: &[&Arc<Vertex>]| {
            expected
                .iter()
                .map(|&vx| (vx.hash(), vx))
                .sorted_by_key(|e| e.0)
                .eq(cx
                    .conflicts_of(dut)
                    .into_iter()
                    .map(|(hash, vx)| (hash.clone(), vx))
                    .sorted_by_key(|e| e.0))
        };
        assert!(should_conflict(&c1, &[&c2, &c4, &c6]));
        assert!(should_conflict(&c3, &[&c2, &c4, &c6]));
        assert!(should_conflict(&c5, &[&c2, &c4, &c6]));
        assert!(should_conflict(&c2, &[&c1, &c3, &c5]));
        assert!(should_conflict(&c4, &[&c1, &c3, &c5]));
        assert!(should_conflict(&c6, &[&c1, &c3, &c5]));
    }
}
