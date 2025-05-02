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

macro_rules! test_graph {
    ([$($input:expr),*]) => {
        {
            TestGraph::new(&[
                $(
                    {
                        let mut parts = $input.split(" -> ");
                        let left = parts.next().unwrap().trim();
                        let rights: Vec<&str> = parts.next().unwrap().split(',').map(|s| s.trim()).collect();
                        (left, rights)
                    }
                ),*
            ])
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
        let mut dag = DAG::with_initial_vertices(
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
    let mut tg = test_graph!([
        "gen ->         ",
        "v0 -> gen     ",
        "v1 -> v0     ",
        "v2 -> v1     ",
        "v3 -> v2     "
    ]);

    // Assert the initial state
    tg.check_state_with_updates(vec![
        ("gen", ACCEPTED),
        ("v0", STRONG_PREF),
        ("v1", STRONG_PREF),
        ("v2", STRONG_PREF),
        ("v3", STRONG_PREF),
    ]);

    // Award chits to everything except
    tg.record_query("v0", true).unwrap();
    tg.record_query("v1", true).unwrap();
    tg.record_query("v2", true).unwrap();

    // Confirm no preferences have changed since last check
    tg.check_state_with_updates(vec![]);

    // v00 should be accepted now
    tg.record_query("v3", true).unwrap();
    tg.check_state_with_updates(vec![("v0", ACCEPTED)]);
}

#[test]
fn small_chain_with_conflicts() {
    let mut tg = test_graph!([
        "gen ->         ",
        "v00 -> gen     ",
        "v01 -> gen     ",
        "a10 -> v00, v01",
        "b10 -> v01, v00", // conflicts with a10
        "v20 -> a10     ",
        "v30 -> v20     ",
        "v40 -> v30     ",
        "v50 -> v40     ",
        "v60 -> v50     ",
        "v70 -> v60     "
    ]);

    // Assert the initial state
    tg.check_state_with_updates(vec![
        ("gen", ACCEPTED),
        ("v00", STRONG_PREF),
        ("v01", STRONG_PREF),
        ("a10", STRONG_PREF),
        ("b10", NO_PREF),
        ("v20", STRONG_PREF),
        ("v30", STRONG_PREF),
        ("v40", STRONG_PREF),
        ("v50", STRONG_PREF),
        ("v60", STRONG_PREF),
        ("v70", STRONG_PREF),
    ]);

    // Award chits to everything except
    tg.record_query("v00", true).unwrap();
    tg.record_query("v01", true).unwrap();
    tg.check_state_with_updates(vec![]); // expect no changes so far

    tg.record_query("b10", true).unwrap(); // vote for b10 initially
    tg.check_state_with_updates(vec![
        ("a10", NO_PREF),
        ("b10", STRONG_PREF),
        ("v20", NO_PREF),
        ("v30", NO_PREF),
        ("v40", NO_PREF),
        ("v50", NO_PREF),
        ("v60", NO_PREF),
        ("v70", NO_PREF),
    ]);
    assert!(tg.record_query("v20", false).is_err()); // waiting on a10
    tg.record_query("a10", false).unwrap();
    tg.check_state_with_updates(vec![]); // should not change state

    tg.record_query("v20", true).unwrap(); // vote for v20 supports a10 in lieu of b10
    tg.check_state_with_updates(vec![
        ("a10", STRONG_PREF),
        ("b10", NO_PREF),
        ("v20", STRONG_PREF),
        ("v30", STRONG_PREF),
        ("v40", STRONG_PREF),
        ("v50", STRONG_PREF),
        ("v60", STRONG_PREF),
        ("v70", STRONG_PREF),
    ]);

    // Vertices v00 & v01 should become accepted at safe early committment criteria
    tg.record_query("v30", true).unwrap();
    tg.check_state_with_updates(vec![]);
    tg.record_query("v40", true).unwrap();
    tg.check_state_with_updates(vec![]);
    tg.record_query("v50", true).unwrap();

    // v00 & v01 should have reached safe early committment
    tg.check_state_with_updates(vec![("v00", ACCEPTED), ("v01", ACCEPTED)]);

    // Since there is a conflict at a10/b10, and b10 received a vote, no vertices at or after that
    // point may be accepte according to the "safe early committment rule". They must wait until
    // reaching ful confidence.
    tg.record_query("v60", true).unwrap();
    tg.check_state_with_updates(vec![]);
    tg.record_query("v70", true).unwrap();
    tg.check_state_with_updates(vec![]);
}

// TODO: Confirm chit is cleared on color flip

#[test]
fn tower_1222() {
    let mut tg = test_graph!([
        "gen ->         ", //      gen
        "v00 -> gen     ", //      / \
        "v01 -> gen     ", //    v00 v01
        "v10 -> v00, v01", //    |  X  |
        "v11 -> v00, v01", //    v10 v11
        "v20 -> v10, v11", //    |  X  |
        "v21 -> v10, v11"  //    v20 v21
    ]);
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
    todo!()
}
