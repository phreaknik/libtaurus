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

const MAX_CONF: usize = 8;
const MAX_COUNT: usize = 4;

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
                max_confidence: MAX_CONF,
                max_count: MAX_COUNT,
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
        // Update the expected state
        for (label, state) in &changes {
            self.expected_state.insert(label, *state);
        }
        // Check all the states against expected
        for (label, expected) in &self.expected_state {
            let actual = self
                .dag
                .query(&self.vertex_by_label[label].hash())
                .expect(&format!("Failed to query {label}"));
            assert_eq!(&actual, expected, "unexpected state for {label}");
        }
    }

    /// Helper to record a query for the specified vertex
    fn record_query(&mut self, label: &'a str, pref: bool) -> Result<(), dag::Error> {
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
        "v01 -> v00     ",
        "v02 -> v01     ",
        "v03 -> v02     "
    ]);

    // Assert the initial state
    tg.check_state_with_updates(vec![
        ("gen", ACCEPTED),
        ("v00", STRONG_PREF),
        ("v01", STRONG_PREF),
        ("v02", STRONG_PREF),
        ("v03", STRONG_PREF),
    ]);

    // Award chits to everything except
    tg.record_query("v00", true).unwrap();
    tg.record_query("v01", true).unwrap();
    tg.record_query("v02", true).unwrap();

    // Confirm no preferences have changed since last check
    tg.check_state_with_updates(vec![]);

    // v00 should be accepted now
    tg.record_query("v03", true).unwrap();
    tg.check_state_with_updates(vec![("v00", ACCEPTED)]);
}

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
