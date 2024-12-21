use libtaurus::{
    consensus::dag::{self, DAG},
    vertex::make_rand_vertex,
    wire::WireFormat,
    Vertex,
};
use std::{collections::HashMap, iter, sync::Arc};

const REJECTED: (bool, bool) = (false, true);
const NO_PREF: (bool, bool) = (false, false);
const STRONG_PREF: (bool, bool) = (true, false);
const ACCEPTED: (bool, bool) = (true, true);

const THRESH_ACCEPT: usize = 8;
const THRESH_SE_ACCEPT: usize = 4;

macro_rules! edges {
    ([$($input:expr),*]) => {
        {
            &[
                $(
                    {
                        let mut parts = $input.split(" -> ");
                        let left = parts.next().unwrap().trim();
                        let rights: Vec<&str> = parts.next().unwrap().split(',').map(|s| s.trim()).collect();
                        (left, rights)
                    }
                ),*
            ]
        }
    };
}

struct TestGraph<'a> {
    dag: DAG,
    vertex_by_label: HashMap<&'a str, Arc<Vertex>>,
    expected_state: HashMap<&'a str, (bool, bool)>,
}

impl<'a> TestGraph<'a> {
    /// Build a [`TestGraph`] from a human readable description of the vertices in the graph.
    fn new(edges: &[(&'a str, Vec<&'a str>)]) -> TestGraph<'a> {
        let mut vertices: HashMap<&str, Arc<Vertex>> = HashMap::new();
        vertices.insert(edges[0].0, Arc::new(Vertex::empty())); // first is genesis
        for (label, parent_labels) in &edges[1..] {
            let parents = parent_labels
                .into_iter()
                .map(|label| {
                    vertices
                        .get(label)
                        .expect(&format!("couldn't find vertex {label}"))
                        .clone()
                })
                .collect::<Vec<_>>();
            vertices.insert(label, make_rand_vertex(&parents));
        }
        let mut dag = DAG::new(
            dag::Config {
                genesis: vertices[edges[0].0].hash(),
                thresh_safe_early_commit: THRESH_SE_ACCEPT,
                thresh_accepted: THRESH_ACCEPT,
            },
            iter::once(&vertices[edges[0].0]),
        )
        .unwrap();
        for (label, _) in &edges[1..] {
            dag.try_insert(&vertices[label]).unwrap();
        }
        for (label, _) in &edges[..] {
            println!(">>>> {label}.hash() = {}", vertices[label].hash());
        }
        TestGraph {
            dag,
            vertex_by_label: vertices,
            expected_state: HashMap::new(),
        }
    }

    /// Insert new vertices to the DAG
    fn extend(&mut self, edges: &[(&'a str, Vec<&'a str>)]) {
        for (label, parent_labels) in &edges[..] {
            let parents = parent_labels
                .into_iter()
                .map(|label| {
                    self.vertex_by_label
                        .get(label)
                        .expect(&format!("couldn't find vertex {label}"))
                        .clone()
                })
                .collect::<Vec<_>>();
            self.vertex_by_label
                .insert(label, make_rand_vertex(&parents));
        }
        for (label, _) in &edges[..] {
            self.dag.try_insert(&self.vertex_by_label[label]).unwrap();
        }
    }

    /// Updates the saved state with the specified expected states, checks that each state in the
    /// dag matches this expected, and saves the state for subsequent executions. If no state
    /// change is expected, calling this function with an empty list of changes, will re-assert the
    /// prior state.
    fn check_state_with_updates(&mut self, changes: Vec<(&'a str, (bool, bool))>) {
        println!(">>>> check states");
        // Update the expected state
        for (label, state) in &changes {
            self.expected_state.insert(label, *state);
        }
        // Check all the states against expected
        for (label, expected) in &self.expected_state {
            println!(">>>> query({label})");
            let actual = self
                .dag
                .query(&self.vertex_by_label[label].hash())
                .expect(&format!("Failed to query {label}"));
            assert_eq!(&actual, expected, "unexpected state for {label}");
        }
    }

    /// Helper to check the frontier matches the expected
    fn check_frontier<L>(&mut self, expected: L)
    where
        L: IntoIterator<Item = &'a str>,
    {
        let f: Vec<Arc<Vertex>> = expected
            .into_iter()
            .map(|label| self.vertex_by_label[label].clone())
            .collect();
        assert_eq!(self.dag.get_frontier(), f);
    }

    /// Helper to record a query for the specified vertex
    fn record_query(&mut self, label: &'a str, pref: bool) -> Result<(), dag::Error> {
        println!(
            ">>>> vote {} {}",
            if pref { "for" } else { "against" },
            label
        );
        self.dag
            .record_query_result(&self.vertex_by_label[label].hash(), pref)
    }
}

// TODO: I think it is possible to get a vertex to be accepted, even though its parents are not...
// test this.

#[test]
fn basic_chain() {
    // Simple list of non-conflicting vertices. Any vertex in the chain should become accepted
    // under the "safe early committment" rule, once MAX_COUNT chits accumulate
    let mut tg = TestGraph::new(edges!([
        "gen ->         ",
        "v0 -> gen     ",
        "v1 -> v0     ",
        "v2 -> v1     ",
        "v3 -> v2     "
    ]));

    // Assert the initial state
    tg.check_state_with_updates(vec![
        ("gen", ACCEPTED),
        ("v0", STRONG_PREF),
        ("v1", STRONG_PREF),
        ("v2", STRONG_PREF),
        ("v3", STRONG_PREF),
    ]);
    tg.check_frontier(["v3"]);

    // Award chits to everything except
    tg.record_query("v0", true).unwrap();
    tg.record_query("v1", true).unwrap();
    tg.record_query("v2", true).unwrap();

    // Confirm no preferences have changed since last check
    tg.check_state_with_updates(vec![]);
    tg.check_frontier(["v3"]);

    // v00 should be accepted now
    tg.record_query("v3", true).unwrap();
    tg.check_state_with_updates(vec![("v0", ACCEPTED)]);
    tg.check_frontier(["v3"]);
}

#[test]
fn small_chain_with_conflicts() {
    let mut tg = TestGraph::new(edges!([
        "gen ->         ",
        "v00 -> gen     ",
        "v01 -> gen     ",
        "a10 -> v00, v01",
        "b10 -> v01, v00", // conflicts with a10
        "a20 -> a10     ",
        "a30 -> a20     ",
        "a40 -> a30     ",
        "a50 -> a40     ",
        "a60 -> a50     ",
        "a70 -> a60     ",
        "a80 -> a70     ",
        "a90 -> a80     "
    ]));

    // Assert the initial state
    tg.check_state_with_updates(vec![
        ("gen", ACCEPTED),
        ("v00", STRONG_PREF),
        ("v01", STRONG_PREF),
        ("a10", STRONG_PREF),
        ("b10", NO_PREF),
        ("a20", STRONG_PREF),
        ("a30", STRONG_PREF),
        ("a40", STRONG_PREF),
        ("a50", STRONG_PREF),
        ("a60", STRONG_PREF),
        ("a70", STRONG_PREF),
        ("a80", STRONG_PREF),
        ("a90", STRONG_PREF),
    ]);
    tg.check_frontier(["a90"]);

    // Award chits to everything except
    tg.record_query("v00", true).unwrap();
    tg.record_query("v01", true).unwrap();
    tg.check_state_with_updates(vec![]); // expect no changes so far
    tg.check_frontier(["a90"]);

    tg.record_query("b10", true).unwrap(); // vote for b10 initially
    tg.check_state_with_updates(vec![
        ("a10", NO_PREF),
        ("b10", STRONG_PREF),
        ("a20", NO_PREF),
        ("a30", NO_PREF),
        ("a40", NO_PREF),
        ("a50", NO_PREF),
        ("a60", NO_PREF),
        ("a70", NO_PREF),
        ("a80", NO_PREF),
        ("a90", NO_PREF),
    ]);
    tg.check_frontier(["b10"]);
    tg.record_query("a10", false).unwrap();
    tg.check_state_with_updates(vec![]); // should not change state
    tg.check_frontier(["b10"]);

    tg.record_query("a20", true).unwrap(); // a10 confidence now tied with b10..
    tg.check_frontier(["b10"]); // no change for tie

    tg.record_query("a30", true).unwrap(); // a10 confidence now greater than b10
    tg.check_state_with_updates(vec![
        ("a10", STRONG_PREF),
        ("b10", NO_PREF),
        ("a20", STRONG_PREF),
        ("a30", STRONG_PREF),
        ("a40", STRONG_PREF),
        ("a50", STRONG_PREF),
        ("a60", STRONG_PREF),
        ("a70", STRONG_PREF),
        ("a80", STRONG_PREF),
        ("a90", STRONG_PREF),
    ]);
    tg.check_frontier(["a90"]);

    // Vertices v00 & v01 should become accepted at safe early committment criteria
    tg.record_query("a50", true).unwrap(); // out of order query should work
    tg.check_state_with_updates(vec![]);
    tg.check_frontier(["a90"]);
    tg.record_query("a40", true).unwrap();
    // v00 & v01 should have reached safe early committment
    tg.check_state_with_updates(vec![("v00", ACCEPTED), ("v01", ACCEPTED)]);
    tg.check_frontier(["a90"]);

    tg.record_query("a30", true).unwrap_err(); // Already inserted
    tg.check_state_with_updates(vec![]);
    tg.check_frontier(["a90"]);

    // Since there is a conflict at a10/b10, and b10 received a vote, no vertices at or after that
    // point may be accepte according to the "safe early committment rule". They must wait until
    // reaching ful confidence.
    tg.record_query("a60", true).unwrap();
    tg.check_state_with_updates(vec![]);
    tg.check_frontier(["a90"]);
    tg.record_query("a70", true).unwrap();
    tg.check_state_with_updates(vec![]);
    tg.check_frontier(["a90"]);
    tg.record_query("a80", true).unwrap();
    tg.check_state_with_updates(vec![]);
    tg.check_frontier(["a90"]);

    // Create large subtree after b10
    tg.extend(edges!([
        "b20 -> b10     ",
        "b21 -> b10     ",
        "b30 -> b20, b21",
        "b40 -> b30     ",
        "b50 -> b40     ",
        "b60 -> b50     ",
        "b70 -> b60     ",
        "b80 -> b70     ",
        "b90 -> b80     ",
        "b100 -> b90    "
    ]));
    tg.check_state_with_updates(vec![
        ("b20", NO_PREF),
        ("b21", NO_PREF),
        ("b30", NO_PREF),
        ("b40", NO_PREF),
        ("b50", NO_PREF),
        ("b60", NO_PREF),
        ("b70", NO_PREF),
        ("b80", NO_PREF),
        ("b90", NO_PREF),
        ("b100", NO_PREF),
    ]);
    tg.check_frontier(["a90"]);

    // Should reach acceptance threshold for conflicts a10/b10. A few after should become accepted
    // under SE criteria
    tg.record_query("a90", true).unwrap();
    tg.check_state_with_updates(vec![
        ("a10", ACCEPTED),
        ("b10", REJECTED),
        ("a20", ACCEPTED),
        ("a30", ACCEPTED),
        ("a40", ACCEPTED),
        ("a50", ACCEPTED),
        ("a60", ACCEPTED),
        ("a70", STRONG_PREF),
        ("a80", STRONG_PREF),
        ("a90", STRONG_PREF),
        ("b20", REJECTED),
        ("b21", REJECTED),
        ("b30", REJECTED),
        ("b40", REJECTED),
        ("b50", REJECTED),
        ("b60", REJECTED),
        ("b70", REJECTED),
        ("b80", REJECTED),
        ("b90", REJECTED),
        ("b100", REJECTED),
    ]);
    tg.check_frontier(["a90"]);
}

#[test]
fn tower_1222() {
    let mut tg = TestGraph::new(edges!([
        "gen ->         ", //      gen
        "v00 -> gen     ", //      / \
        "v01 -> gen     ", //    v00 v01
        "v10 -> v00, v01", //    |  X  |
        "v11 -> v00, v01", //    v10 v11
        "v20 -> v10, v11", //    |  X  |
        "v21 -> v10, v11"  //    v20 v21
    ]));
    // Assert the initial state
    tg.check_state_with_updates(vec![
        ("gen", ACCEPTED),
        ("v00", STRONG_PREF),
        ("v01", STRONG_PREF),
        ("v10", STRONG_PREF),
        ("v11", STRONG_PREF),
        ("v20", STRONG_PREF),
        ("v21", STRONG_PREF),
    ]);
    tg.check_frontier(["v20", "v21"]);

    tg.record_query("v00", true).unwrap();
    tg.check_state_with_updates(vec![]);
    tg.check_frontier(["v20", "v21"]);
    tg.record_query("v01", true).unwrap();
    tg.check_state_with_updates(vec![]);
    tg.check_frontier(["v20", "v21"]);
    tg.record_query("v10", true).unwrap();
    tg.check_state_with_updates(vec![]);
    tg.check_frontier(["v20", "v21"]);
    tg.record_query("v11", true).unwrap();
    tg.check_state_with_updates(vec![]);
    tg.check_frontier(["v20", "v21"]);
    tg.record_query("v20", true).unwrap();
    tg.check_state_with_updates(vec![("v00", ACCEPTED), ("v01", ACCEPTED)]);
    tg.check_frontier(["v20", "v21"]);
    tg.record_query("v21", true).unwrap();
    tg.check_state_with_updates(vec![]);
    tg.check_frontier(["v20", "v21"]);
}

#[test]
fn overtaker() {
    let mut tg = TestGraph::new(edges!([
        "gen ->         ", //      gen
        "v00 -> gen     ", //      /  \
        "v01 -> gen     ", //    v00  v01
        "a1 -> v00, v01 ", //    |  X  |
        "b1 -> v01, v00 ", //    a1   b1 <--- conflict
        "b2 -> b1       ", //         b2
        "b3 -> b2       ", //         b3
        "b4 -> b3       ", //         b4
        "b5 -> b4       ", //         b5
        "b6 -> b5       ", //         b6
        "b7 -> b6       ", //         b7
        "b8 -> b7       "  //         b8
    ]));
    // Assert the initial state
    tg.check_state_with_updates(vec![
        ("gen", ACCEPTED),
        ("v00", STRONG_PREF),
        ("v01", STRONG_PREF),
        ("a1", STRONG_PREF),
        ("b1", NO_PREF),
        ("b2", NO_PREF),
        ("b3", NO_PREF),
        ("b4", NO_PREF),
        ("b5", NO_PREF),
        ("b6", NO_PREF),
        ("b7", NO_PREF),
        ("b8", NO_PREF),
    ]);
    tg.check_frontier(["a1"]);

    // Strengthen a1 with chits
    tg.record_query("v00", true).unwrap();
    tg.record_query("v01", true).unwrap();
    tg.record_query("a1", true).unwrap();
    tg.record_query("b1", true).unwrap();
    tg.check_state_with_updates(vec![]); // No change
    tg.check_frontier(["a1"]);
    tg.record_query("b2", true).unwrap(); // Should overtake a1
    tg.check_state_with_updates(vec![
        ("v00", ACCEPTED),
        ("v01", ACCEPTED),
        ("a1", NO_PREF),
        ("b1", STRONG_PREF),
        ("b2", STRONG_PREF),
        ("b3", STRONG_PREF),
        ("b4", STRONG_PREF),
        ("b5", STRONG_PREF),
        ("b6", STRONG_PREF),
        ("b7", STRONG_PREF),
        ("b8", STRONG_PREF),
    ]);
    tg.check_frontier(["b8"]);
    tg.record_query("b3", true).unwrap();
    tg.record_query("b4", true).unwrap();
    tg.record_query("b5", true).unwrap();
    tg.record_query("b6", true).unwrap();
    tg.record_query("b7", true).unwrap();
    tg.check_state_with_updates(vec![]); // No change
    tg.check_frontier(["b8"]);

    // After inserting b8, b1 and several descendents should become accepted
    tg.record_query("b8", true).unwrap();
    tg.check_state_with_updates(vec![
        ("v00", ACCEPTED),
        ("v01", ACCEPTED),
        ("a1", REJECTED),
        ("b1", ACCEPTED),
        ("b2", ACCEPTED),
        ("b3", ACCEPTED),
        ("b4", ACCEPTED),
        ("b5", ACCEPTED),
        ("b6", STRONG_PREF),
        ("b7", STRONG_PREF),
        ("b8", STRONG_PREF),
    ]);
    tg.check_frontier(["b8"]);
}

#[test]
fn resetter() {
    let mut tg = TestGraph::new(edges!([
        "gen ->         ",
        "v0 -> gen      ",
        "v1 -> v0       ",
        "v2 -> v1       ",
        "v3 -> v2       ", // <- vote against to reset count
        "v4 -> v3       ",
        "v5 -> v4       ",
        "v6 -> v5       ",
        "v7 -> v6       "
    ]));
    // Assert the initial state
    tg.check_state_with_updates(vec![
        ("gen", ACCEPTED),
        ("v0", STRONG_PREF),
        ("v1", STRONG_PREF),
        ("v2", STRONG_PREF),
        ("v3", STRONG_PREF),
        ("v4", STRONG_PREF),
        ("v5", STRONG_PREF),
        ("v6", STRONG_PREF),
        ("v7", STRONG_PREF),
    ]);
    tg.check_frontier(["v7"]);
    tg.record_query("v0", true).unwrap();
    tg.record_query("v1", true).unwrap();
    tg.record_query("v2", true).unwrap();
    tg.check_state_with_updates(vec![]); // No change
    tg.check_frontier(["v7"]);
    tg.record_query("v3", false).unwrap(); // vote against v3 to reset count
    tg.check_state_with_updates(vec![]); // No change
    tg.check_frontier(["v7"]);
    tg.record_query("v4", true).unwrap();
    tg.check_state_with_updates(vec![]); // No change
    tg.check_frontier(["v7"]);
    tg.record_query("v5", true).unwrap();
    tg.check_state_with_updates(vec![]); // No change
    tg.check_frontier(["v7"]);
    tg.record_query("v6", true).unwrap();
    tg.check_state_with_updates(vec![]); // No change
    tg.check_frontier(["v7"]);

    // After v7, v0-v4 should become accepted
    tg.record_query("v7", true).unwrap();
    tg.check_state_with_updates(vec![
        ("v0", ACCEPTED),
        ("v1", ACCEPTED),
        ("v2", ACCEPTED),
        ("v3", ACCEPTED),
        ("v4", ACCEPTED),
    ]);
    tg.check_frontier(["v7"]);
}

// TODO: need test to ensure a stalled vertex (vertex with non-virtuous parents) gets retried with
// new parents. See "dynamic parent selection" in Avalanche consensus paper.
