use libtaurus::{
    consensus::dag::{self, Status, DAG},
    vertex::make_rand_vertex,
    wire::WireFormat,
    Vertex,
};
use std::{collections::HashMap, iter, sync::Arc};

const REJECTED: Status = Status::Rejected;
const NOT_PREF: Status = Status::NotPreferred;
const WEAK_PREF: Status = Status::StronglyPreferred;
const STRONG_PREF: Status = Status::StronglyPreferred;
const ACCEPTED: Status = Status::Accepted;

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
    expected_state: HashMap<&'a str, (Status, Vec<(bool, Status)>)>,
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
                max_confidence: 9,
                max_count: 9,
            },
            iter::once(&vertices[edges[0].0]),
        )
        .unwrap();
        for (label, _) in &edges[1..] {
            dag.try_insert(&vertices[label]).unwrap();
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
    fn assert_state(&mut self, changes: Vec<(&'a str, (Status, Vec<(bool, Status)>))>) {
        // Update the expected state
        for (label, states) in &changes {
            self.expected_state.insert(label, states.clone());
        }
        // Check all the states against expected
        for (label, (v_state, pc_states)) in changes {
            let states = self
                .dag
                .query(&self.vertex_by_label[label].hash())
                .expect(&format!("Failed to query {label}"));
            assert_eq!(
                states.1, *pc_states,
                "unexpected parent constraint states of {label}"
            );
            assert_eq!(states.0, v_state, "unexpected vertex state of {label}");
        }
    }
}

// TODO: I think it is possible to get a vertex to be accepted, even though its parents are not...
// test this.

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
    tg.assert_state(vec![
        ("gen", (ACCEPTED, vec![])),
        ("v00", (STRONG_PREF, vec![(false, ACCEPTED)])),
        ("v01", (STRONG_PREF, vec![(false, ACCEPTED)])),
        (
            "v10",
            (
                STRONG_PREF,
                vec![
                    (false, STRONG_PREF),
                    (false, STRONG_PREF),
                    (false, STRONG_PREF),
                ],
            ),
        ),
        (
            "v11",
            (
                STRONG_PREF,
                vec![
                    (false, STRONG_PREF),
                    (false, STRONG_PREF),
                    (false, STRONG_PREF),
                ],
            ),
        ),
        (
            "v20",
            (
                STRONG_PREF,
                vec![
                    (false, STRONG_PREF),
                    (false, STRONG_PREF),
                    (false, STRONG_PREF),
                ],
            ),
        ),
        (
            "v21",
            (
                STRONG_PREF,
                vec![
                    (false, STRONG_PREF),
                    (false, STRONG_PREF),
                    (false, STRONG_PREF),
                ],
            ),
        ),
    ]);
    todo!()
}
