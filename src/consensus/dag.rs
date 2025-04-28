use crate::{
    params,
    vertex::{self, Constraint},
    Vertex, VertexHash, WireFormat,
};
use itertools::Itertools;
use std::{
    collections::{HashMap, HashSet},
    iter::once,
    result,
    sync::Arc,
};
use tracing::error;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("already exists")]
    AlreadyExists,
    #[error("bad height")]
    BadHeight(u64, u64),
    #[error("bad parent height")]
    BadParentHeight,
    #[error(transparent)]
    Hash(#[from] crate::hash::Error),
    #[error("missing parents")]
    MissingParents(Vec<VertexHash>),
    #[error("no parents")]
    NoParents,
    #[error("not found")]
    NotFound,
    #[error(transparent)]
    ProstDecode(#[from] prost::DecodeError),
    #[error(transparent)]
    ProstEncode(#[from] prost::EncodeError),
    #[error(transparent)]
    Vertex(#[from] vertex::Error),
    #[error("waiting on parents to process")]
    WaitingOnParents(Vec<VertexHash>),
}
type Result<T> = result::Result<T, Error>;

/// Configuraion parameters for a [`DAG`] instance
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    acceptance_threshold: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            acceptance_threshold: params::AVALANCHE_ACCEPTANCE_THRESHOLD,
        }
    }
}

/// Implementation of SESAME DAG. Each [`Vertex`] represents an event in the event graph. Each
/// [`Vertex`] commits to a set of ordering [`Constraint`]s for each pair of parent vertices. We
/// arrange these ordering constraints into an independent graph, and use Avalanche consensus to
/// reach agreement on the total ordering of event vertices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DAG {
    /// Configuration parameters
    config: Config,

    /// Map of every vertex in the event graph
    vertex: HashMap<VertexHash, Arc<Vertex>>,

    /// Map of known children for each vertex in the event graph
    children: HashMap<VertexHash, HashSet<VertexHash>>,

    /// Map of each vertex to the list of ordering constraints applied to it
    constraints: HashMap<VertexHash, HashSet<Constraint>>,

    /// Map of each ordering constraint to its corresponding Avalanche state variables
    state: HashMap<Constraint, AvalancheState>,

    /// Vertices which define the frontier of the graph.
    /// The frontier is ordered according to latest ordering preference
    frontier: HashSet<VertexHash>,
}

impl DAG {
    /// Create a new [`DAG`] with the given [`Config`]
    pub fn new(config: Config) -> DAG {
        DAG {
            config,
            vertex: HashMap::new(),
            children: HashMap::new(),
            constraints: HashMap::new(),
            state: HashMap::new(),
            frontier: HashSet::new(),
        }
    }

    /// Before inserting a vertex into the [`DAG`] it must pass these checks
    fn check_vertex(&self, vx: &Vertex) -> Result<()> {
        // Rule out any obviously insane vertices
        vx.sanity_checks()?;

        // Check the child map to make sure this vertex doesn't already exist
        if self.children.contains_key(&vx.hash()) {
            return Err(Error::AlreadyExists);
        }

        // Collect any parent vertices which we do not have yet, or which we are still waiting to
        // be processed
        let mut missing = Vec::with_capacity(vx.parents.len());
        let mut waiting = Vec::with_capacity(vx.parents.len());
        for vhash in &vx.parents {
            if !self.children.contains_key(vhash) {
                missing.push(*vhash);
            } else if !self.vertex.contains_key(vhash) {
                waiting.push(*vhash);
            }
        }
        if !missing.is_empty() {
            return Err(Error::MissingParents(missing));
        }
        if !waiting.is_empty() {
            return Err(Error::WaitingOnParents(waiting));
        }

        // Confirm the heights of each parent match
        let parent_height = self.vertex[vx.parents.first().unwrap()].height;
        if !vx
            .parents
            .iter()
            .all(|p| self.vertex[p].height == parent_height)
        {
            return Err(Error::BadParentHeight);
        }

        // Confirm vertex height is 1 + parent height
        let expected_height = parent_height + 1;
        if expected_height != vx.height {
            return Err(Error::BadHeight(vx.height, expected_height));
        }

        Ok(())
    }

    /// Add this [`Vertex`] as a known child to each of its parents
    fn map_child(&mut self, vhash: &VertexHash, parents: &[VertexHash]) {
        for parent in parents {
            if let Some(children) = self.children.get_mut(parent) {
                children.insert(*vhash);
            } else {
                self.children.insert(*parent, once(*vhash).collect());
            }
        }
    }

    /// Insert a [`Vertex`] without checking. Returns boolean indicating if the vertex is preferred
    /// or not, as well as a list of known children waiting to be inserted.
    fn insert_unchecked(&mut self, vx: &Arc<Vertex>) -> (bool, Option<HashSet<VertexHash>>) {
        // Add vertex to list of inserted vertices
        self.vertex.try_insert(vx.hash(), vx.clone()).unwrap();

        // Add an entry in the child map, if it doesn't already exist
        if !self.children.contains_key(&vx.hash()) {
            self.children.insert(vx.hash(), HashSet::new());
        }

        // Add an entry in the constraint map, if it doesn't already exist
        let constraints = vx
            .parent_constraints()
            .inspect(|c| {
                // Add an entry in the state map, if it doesn't already exist
                if !self.state.contains_key(c) {
                    self.state.insert(*c, AvalancheState::default());
                }

                // Add an entry to the constraint map of every vertex constrained by these
                self.constraints.get_mut(&c.0).unwrap().insert(*c);
                self.constraints.get_mut(&c.1).unwrap().insert(*c);
            })
            .collect();
        self.constraints.insert(vx.hash(), constraints);

        // Update child and progeny maps
        self.map_child(&vx.hash(), &vx.parents);

        (
            self.is_preferred(vx).unwrap(),
            self.children.get(&vx.hash()).cloned(),
        )
    }

    /// Insert a vertex into the [`DAG`]. Returns boolean indicating
    /// if the vertex is preferred or not, as well as a list of known children waiting to be
    /// inserted.
    pub fn try_insert(&mut self, vx: &Arc<Vertex>) -> Result<(bool, Option<HashSet<VertexHash>>)> {
        // TODO: this should be thread safe...need mutex on DAG?
        // Check if the vertex may be inserted
        self.check_vertex(vx).map_err(|e| {
            // Add vertex as known child, even if we are missing some of its parents
            if let Error::MissingParents(_) = e {
                self.map_child(&vx.hash(), &vx.parents);
            }
            e
        })?;

        Ok(self.insert_unchecked(vx))
    }

    /// Return true if the specified vertex is preferred over all its conflicts
    pub fn is_preferred(&self, _vx: &Vertex) -> Result<bool> {
        todo!()
    }

    /// Award a chit to the specified vertex, according to the Avalanche protocol
    pub fn award_chit(&mut self, vhash: &VertexHash) -> Result<()> {
        // Helper to recursively reset confidences. Returns true if the confidence of the specified
        // constraint has changed.
        fn reset(dag: &mut DAG, c: &Constraint) -> bool {
            // Reset self chit & confidence for this vertex
            let (conf_changed, children_to_reset) = dag
                .state
                .get_mut(&c)
                .and_then(|state| {
                    let orig_conf = state.confidence;
                    state.chit = false;
                    state.confidence = 0;
                    state.preferred = false;
                    Some((orig_conf != state.confidence, state.children.clone()))
                })
                .unwrap();

            // Recursively reset each child
            for child in &children_to_reset {
                reset(dag, child);
            }

            conf_changed
        }

        // Helper to recursively recompute confidences
        fn recompute_at(dag: &mut DAG, c: &Constraint) {
            // Sum up child confidences
            let orig_state = dag.state[c].clone();
            let child_conf = orig_state
                .children
                .iter()
                .map(|child| dag.state[child].confidence)
                .sum::<usize>();

            // Assign new confidence as chit + child_conf
            let new_conf = dag
                .state
                .get_mut(c)
                .and_then(|state| {
                    state.confidence = state.chit as usize + child_conf;
                    Some(state.confidence)
                })
                .unwrap();

            if orig_state.confidence != new_conf {
                //  Check to see if it has overtaken the confidence of the conflicting/opposite
                // constraint.
                let mut need_recompute = orig_state.parents.clone();
                if dag.state[&c.opposite()].confidence < new_conf && reset(dag, &c.opposite()) {
                    need_recompute.extend(&dag.state[&c.opposite()].parents);
                }

                // Recursively recompute confidences for each constraint which depends on modified
                // the newl constraints.
                for c in &need_recompute {
                    recompute_at(dag, c);
                }
            }
        }

        // Award each constraint a chit
        let modified = self
            .vertex
            .get(vhash)
            .ok_or(Error::NotFound)?
            .parent_constraints()
            .filter_map(|c| {
                let state = self.state.get_mut(&c).unwrap();
                let orig_chit = state.chit;
                let orig_pref = state.preferred;
                state.chit = true;
                state.preferred = true;
                if !orig_chit || !orig_pref {
                    Some(c)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        // Recompute state for each constraint with a new chit
        for c in modified {
            recompute_at(self, &c);
        }

        Ok(())
    }

    /// Get the latest vertices which have no children, in the order we've observed them
    pub fn get_frontier(&mut self) -> Vec<VertexHash> {
        self.frontier
            .iter()
            .map(|vhash| (vhash, self.vertex[vhash].timestamp))
            .sorted_by_key(|(_vhash, time)| *time)
            .map(|(vhash, _time)| vhash)
            .copied()
            .collect()
    }
}

/// State variables used in Avalanche consensus, to reach agreement on the [`Constraint`] graph.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
struct AvalancheState {
    /// A chit is awarded if enough peers preferred this vertex when queried
    chit: bool,

    /// Confidence counter represents the number of successive chits awarded in the progeny
    confidence: usize,

    /// Is this constraint preferred over its conflict constraint? Since a constraint only commits
    /// to the relative order of two vertices, a constraint may only conflict with its
    /// opposite.
    preferred: bool,

    /// Ordering constraints does this constraint depend on
    parents: HashSet<Constraint>,

    /// Known ordering constraints which depend on this
    children: HashSet<Constraint>,
}

#[cfg(test)]
mod test {
    use super::{Config, DAG};
    use crate::{consensus::dag, params, vertex::test_vertex, Vertex, WireFormat};
    use std::{
        assert_matches::assert_matches,
        collections::{HashMap, HashSet},
        sync::Arc,
    };

    #[test]
    fn default_config() {
        let dflt = Config::default();
        assert_eq!(
            dflt.acceptance_threshold,
            params::AVALANCHE_ACCEPTANCE_THRESHOLD
        );
    }

    #[test]
    fn new_dag() {
        let dag = DAG::new(Config::default());
        assert_eq!(dag.config, Config::default());
        assert_eq!(dag.vertex, HashMap::new());
        assert_eq!(dag.children, HashMap::new());
        assert_eq!(dag.constraints, HashMap::new());
        assert_eq!(dag.state, HashMap::new());
        assert_eq!(dag.frontier, HashSet::new());
    }

    #[test]
    fn check_vertex() {
        let gen = Arc::new(Vertex::empty());
        let c0 = test_vertex([&gen]);
        let c1 = test_vertex([&gen]);
        let c2 = test_vertex([&gen]);
        let c3 = test_vertex([&c1, &c2]);
        let mut dag = DAG::new(Config::default());

        dag.vertex.insert(gen.hash(), gen.clone());
        dag.children.insert(gen.hash(), HashSet::new()); // Genesis is "processed"
        dag.children.insert(c0.hash(), HashSet::new());
        dag.vertex.insert(c0.hash(), c0.clone()); // c0 is "processed"
        assert_matches!(dag.check_vertex(&c0), Err(dag::Error::AlreadyExists));
        match dag.check_vertex(&c3) {
            Err(dag::Error::MissingParents(missing)) => {
                assert_eq!(missing, [c1.hash(), c2.hash()])
            }
            _ => panic!("Expected Error::MissingParents"),
        }
        dag.children.insert(c1.hash(), HashSet::new()); // c1 is known but not processed
        match dag.check_vertex(&c3) {
            Err(dag::Error::MissingParents(missing)) => {
                assert_eq!(missing, [c2.hash()])
            }
            _ => panic!("Expected Error::MissingParents"),
        }
        dag.children.insert(c2.hash(), HashSet::new()); // c2 is known but not processed
        match dag.check_vertex(&c3) {
            Err(dag::Error::WaitingOnParents(waiting)) => {
                assert_eq!(waiting, [c1.hash(), c2.hash()])
            }
            _ => panic!("Expected Error::WaitingOnParents"),
        }
        dag.children.insert(c1.hash(), HashSet::new());
        dag.vertex.insert(c1.hash(), c1.clone()); // c1 is "processed"
        match dag.check_vertex(&c3) {
            Err(dag::Error::WaitingOnParents(waiting)) => {
                assert_eq!(waiting, [c2.hash()])
            }
            _ => panic!("Expected Error::WaitingOnParents"),
        }
        dag.children.insert(c2.hash(), HashSet::new());
        dag.vertex.insert(c2.hash(), c2.clone()); // c2 is "processed"
        assert_matches!(dag.check_vertex(&c3), Ok(()));

        let c4 = test_vertex([&gen, &c1]); // Parents have mismatched height
        assert_matches!(dag.check_vertex(&c4), Err(dag::Error::BadParentHeight));

        dag.vertex.insert(c3.hash(), c3.clone());
        dag.children.insert(c3.hash(), HashSet::new()); // c3 is "processed"
        let mut c5 = Vertex::empty().with_parents([&c3]).unwrap();
        c5.height = 2; // Should have height 3
        match dag.check_vertex(&Arc::new(c5)) {
            Err(dag::Error::BadHeight(actual, expected)) => {
                assert_eq!(actual, 2);
                assert_eq!(expected, 3);
            }
            _ => panic!("Expected Error::BadHeight"),
        }
    }

    #[test]
    fn map_child() {
        let gen = Arc::new(Vertex::empty());
        let c0 = test_vertex([&gen]);
        let c1 = test_vertex([&gen]);
        let c2 = test_vertex([&c0, &c1]);
        let mut dag = DAG::new(Config::default());
        dag.children.insert(gen.hash(), HashSet::new());
        dag.children.insert(c0.hash(), HashSet::new());
        dag.children.insert(c1.hash(), HashSet::new());
        dag.children.insert(c2.hash(), HashSet::new());
        assert_eq!(dag.children[&gen.hash()].len(), 0);
        assert_eq!(dag.children[&c0.hash()].len(), 0);
        assert_eq!(dag.children[&c1.hash()].len(), 0);
        assert_eq!(dag.children[&c2.hash()].len(), 0);
        dag.map_child(&c0.hash(), &c0.parents);
        assert_eq!(dag.children[&gen.hash()].len(), 1);
        assert_eq!(dag.children[&c0.hash()].len(), 0);
        assert_eq!(dag.children[&c1.hash()].len(), 0);
        assert_eq!(dag.children[&c2.hash()].len(), 0);
        dag.map_child(&c1.hash(), &c1.parents);
        assert_eq!(dag.children[&gen.hash()].len(), 2);
        assert_eq!(dag.children[&c0.hash()].len(), 0);
        assert_eq!(dag.children[&c1.hash()].len(), 0);
        assert_eq!(dag.children[&c2.hash()].len(), 0);
        dag.map_child(&c2.hash(), &c2.parents);
        assert_eq!(dag.children[&gen.hash()].len(), 2);
        assert_eq!(dag.children[&c0.hash()].len(), 1);
        assert_eq!(dag.children[&c1.hash()].len(), 1);
        assert_eq!(dag.children[&c2.hash()].len(), 0);
    }

    #[test]
    fn insert_unchecked() {
        todo!()
    }

    #[test]
    fn try_insert() {
        todo!()
    }

    #[test]
    fn is_preferred() {
        todo!()
    }

    #[test]
    fn award_chit() {
        todo!()
        //        // Set up test vertices and dag
        //        let gen = Arc::new(Vertex::empty());
        //        let a00 = test_vertex([&gen]);
        //        let a01 = test_vertex([&gen]);
        //        let a02 = test_vertex([&gen]);
        //
        //        // Create conflicting sets b & c
        //        let b10 = test_vertex([&a00, &a01]);
        //        let b11 = test_vertex([&a00, &a02]);
        //        let b20 = test_vertex([&b10]);
        //        let b21 = test_vertex([&b10, &b11]);
        //        let b30 = test_vertex([&b21, &b20]);
        //        let c10 = test_vertex([&a01, &a00]);
        //        let c11 = test_vertex([&a02, &a00]);
        //        let c20 = test_vertex([&c10]);
        //        let c21 = test_vertex([&c10, &c11]);
        //        let c30 = test_vertex([&c21, &c20]);
        //
        //        // Helper to advance the DAG with new vertices
        //        let add_to_dag = |dag: &mut DAG, vx: &Arc<Vertex>| {
        //            dag.vertex.insert(vx.hash(), vx.clone());
        //            dag.children.insert(vx.hash(), HashSet::new());
        //            dag.map_child(&vx.hash(), &vx.parents);
        //        };
        //
        //        // Helper to assert that all confidences match expected
        //        let assert_confs_and_prefs = |dag: &DAG, expected: &[(&Arc<Vertex>, (usize,
        // bool))]| {            // Print actual states
        //            expected
        //                .iter()
        //                .map(|(vx, _)| {
        //                    (
        //                        vx.hash(),
        //                        dag.chitconf[&vx.hash()].0,
        //                        dag.chitconf[&vx.hash()].1,
        //                        dag.preferences[&vx.hash()],
        //                    )
        //                })
        //                .for_each(|(vhash, chit, conf, pref)| {
        //                    println!("state({vhash}) = ({chit}, {conf}, {pref})");
        //                });
        //            // Confirm actual states match expected states
        //            for (vhash, conf, pref) in expected
        //                .iter()
        //                .map(|(vx, confpref)| (vx.hash(), confpref.0, confpref.1))
        //            {
        //                assert_eq!(dag.chitconf[&vhash].1, conf);
        //                assert_eq!(dag.preferences[&vhash], pref);
        //            }
        //        };
        //
        //        // Helper to set the confidence of a vertex
        //        let set_chit = |dag: &mut DAG, vx: &Arc<Vertex>, chit: usize| {
        //            dag.chitconf.get_mut(&vx.hash()).unwrap().0 = chit;
        //            if chit == 0 {
        //                // If we are resetting this vertex, also clear its confidence and
        // preference                dag.chitconf.get_mut(&vx.hash()).unwrap().1 = 0;
        //                *dag.preferences.get_mut(&vx.hash()).unwrap() = false;
        //            }
        //        };
        //
        //        // Insert everything into the DAG
        //        const MAX_CONF: usize = 9;
        //        let mut dag = DAG::new(Config {
        //            acceptance_threshold: MAX_CONF,
        //        });
        //        add_to_dag(&mut dag, &gen);
        //        add_to_dag(&mut dag, &a00);
        //        add_to_dag(&mut dag, &a01);
        //        add_to_dag(&mut dag, &a02);
        //        add_to_dag(&mut dag, &b10);
        //        add_to_dag(&mut dag, &b11);
        //        add_to_dag(&mut dag, &b20);
        //        add_to_dag(&mut dag, &b21);
        //        add_to_dag(&mut dag, &b30);
        //        add_to_dag(&mut dag, &c10);
        //        add_to_dag(&mut dag, &c11);
        //        add_to_dag(&mut dag, &c20);
        //        add_to_dag(&mut dag, &c21);
        //        add_to_dag(&mut dag, &c30);
        //
        //        // Basic test
        //        set_chit(&mut dag, &gen, 1); // High enough to always be preferred
        //        dag.recompute_at(&gen);
        //        assert_eq!(dag.preferences[&gen.hash()], true);
        //        assert_eq!(dag.chitconf[&gen.hash()], (1, 1));
        //        assert_confs_and_prefs(
        //            &dag,
        //            &[
        //                (&gen, (1, true)),
        //                (&a00, (0, false)),
        //                (&a01, (0, false)),
        //                (&a02, (0, false)),
        //                (&b10, (0, false)),
        //                (&b11, (0, false)),
        //                (&b20, (0, false)),
        //                (&b21, (0, false)),
        //                (&b30, (0, false)),
        //                (&c10, (0, false)),
        //                (&c11, (0, false)),
        //                (&c20, (0, false)),
        //                (&c21, (0, false)),
        //                (&c30, (0, false)),
        //            ],
        //        );
        //
        //        // Award chits to "b" sub tree
        //        set_chit(&mut dag, &a00, 1);
        //        set_chit(&mut dag, &a01, 1);
        //        set_chit(&mut dag, &a02, 1);
        //        set_chit(&mut dag, &b10, 1);
        //        set_chit(&mut dag, &b11, 1);
        //        set_chit(&mut dag, &b20, 1);
        //        set_chit(&mut dag, &b21, 1);
        //        set_chit(&mut dag, &b30, 1);
        //        println!(":::: b30 = {}", &b30.hash());
        //        dag.recompute_at(&b30);
        //        assert_confs_and_prefs(
        //            &dag,
        //            &[
        //                (&gen, (9, true)),
        //                (&a00, (6, true)),
        //                (&a01, (5, true)),
        //                (&a02, (4, true)),
        //                (&b10, (4, true)),
        //                (&b11, (3, true)),
        //                (&b20, (2, true)),
        //                (&b21, (2, true)),
        //                (&b30, (1, true)),
        //                (&c10, (0, false)),
        //                (&c11, (0, false)),
        //                (&c20, (0, false)),
        //                (&c21, (0, false)),
        //                (&c30, (0, false)),
        //            ],
        //        );
        //
        //        // Awarding chits to "c" sub tree instead
        //        set_chit(&mut dag, &b10, 0);
        //        set_chit(&mut dag, &b11, 0);
        //        set_chit(&mut dag, &b20, 0);
        //        set_chit(&mut dag, &b21, 0);
        //        set_chit(&mut dag, &b30, 0);
        //        set_chit(&mut dag, &c10, 1);
        //        set_chit(&mut dag, &c11, 1);
        //        set_chit(&mut dag, &c20, 1);
        //        set_chit(&mut dag, &c21, 1);
        //        set_chit(&mut dag, &c30, 1);
        //        println!(":::: c30 = {}", &c30.hash());
        //        dag.recompute_at(&c30);
        //        assert_confs_and_prefs(
        //            &dag,
        //            &[
        //                (&gen, (9, true)),
        //                (&a00, (6, true)),
        //                (&a01, (5, true)),
        //                (&a02, (4, true)),
        //                (&b10, (0, false)),
        //                (&b11, (0, false)),
        //                (&b20, (0, false)),
        //                (&b21, (0, false)),
        //                (&b30, (0, false)),
        //                (&c10, (4, true)),
        //                (&c11, (3, true)),
        //                (&c20, (2, true)),
        //                (&c21, (2, true)),
        //                (&c30, (1, true)),
        //            ],
        //        );
        //
        //        // Extend progeny of b30 to flip "b" sub tree back into preference
        //        let b40 = test_vertex([&b30]);
        //        let b50 = test_vertex([&b40]);
        //        let b60 = test_vertex([&b50]);
        //        let b70 = test_vertex([&b60]);
        //        let b80 = test_vertex([&b70]);
        //        add_to_dag(&mut dag, &b40);
        //        add_to_dag(&mut dag, &b50);
        //        add_to_dag(&mut dag, &b60);
        //        add_to_dag(&mut dag, &b70);
        //        add_to_dag(&mut dag, &b80);
        //        set_chit(&mut dag, &b40, 1);
        //        set_chit(&mut dag, &b50, 1);
        //        set_chit(&mut dag, &b60, 1);
        //        set_chit(&mut dag, &b70, 1);
        //        set_chit(&mut dag, &b80, 1);
        //        println!(":::: b80 = {}", &b80.hash());
        //        dag.recompute_at(&b80);
        //        println!(
        //            ":::: gen.progeny = len={}, w/chits={}",
        //            dag.progeny[&gen.hash()].len(),
        //            dag.progeny[&gen.hash()]
        //                .iter()
        //                .filter(|vhash| dag.chitconf[vhash].0 != 0)
        //                .count()
        //        );
        //        println!(
        //            ":::: a00.progeny = len={}, w/chits={}",
        //            dag.progeny[&a00.hash()].len(),
        //            dag.progeny[&a00.hash()]
        //                .iter()
        //                .filter(|vhash| dag.chitconf[vhash].0 != 0)
        //                .count()
        //        );
        //        println!(
        //            ":::: a01.progeny = len={}, w/chits={}",
        //            dag.progeny[&a01.hash()].len(),
        //            dag.progeny[&a01.hash()]
        //                .iter()
        //                .filter(|vhash| dag.chitconf[vhash].0 != 0)
        //                .count()
        //        );
        //        println!(
        //            ":::: a02.progeny = len={}, w/chits={}",
        //            dag.progeny[&a02.hash()].len(),
        //            dag.progeny[&a02.hash()]
        //                .iter()
        //                .filter(|vhash| dag.chitconf[vhash].0 != 0)
        //                .count()
        //        );
        //        assert_confs_and_prefs(
        //            &dag,
        //            &[
        //                (&gen, (MAX_CONF, true)),
        //                (&a00, (6, true)),
        //                (&a01, (6, true)),
        //                (&a02, (6, true)),
        //                (&b10, (5, true)),
        //                (&b11, (5, true)),
        //                (&b20, (5, true)),
        //                (&b21, (5, true)),
        //                (&b30, (5, true)),
        //                (&c10, (0, false)),
        //                (&c11, (0, false)),
        //                (&c20, (0, false)),
        //                (&c21, (0, false)),
        //                (&c30, (0, false)),
        //                (&b40, (5, true)),
        //                (&b50, (4, true)),
        //                (&b60, (3, true)),
        //                (&b70, (2, true)),
        //                (&b80, (1, true)),
        //            ],
        //        );
    }

    #[test]
    fn get_frontier() {
        todo!()
    }
}
