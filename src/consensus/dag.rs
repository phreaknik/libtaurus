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
    #[error("already inserted")]
    AlreadyInserted,
    #[error("bad height")]
    BadHeight(u64, u64),
    #[error("bad parent height")]
    BadParentHeight,
    #[error("parents conflict")]
    ConflictingParents,
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

    /// Map of every known undecided vertex
    vertex: HashMap<VertexHash, Arc<Vertex>>,

    /// Set of vertices which are in the active region of the graph
    graph: HashSet<VertexHash>,

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
            graph: HashSet::new(),
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
        if self.graph.contains(&vx.hash()) {
            return Err(Error::AlreadyInserted);
        }

        // Collect any parent vertices which we do not have yet, or which we are still waiting to
        // be processed
        let mut missing = Vec::with_capacity(vx.parents.len());
        let mut waiting = Vec::with_capacity(vx.parents.len());
        for vhash in &vx.parents {
            if !self.vertex.contains_key(vhash) {
                missing.push(*vhash);
            } else if !self.graph.contains(vhash) {
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
    fn insert_unchecked(&mut self, vx: &Arc<Vertex>) {
        // Add vertex to list of inserted vertices
        self.vertex.insert(vx.hash(), vx.clone());

        // Add an entry in the child map, if it doesn't already exist
        if !self.children.contains_key(&vx.hash()) {
            self.children.insert(vx.hash(), HashSet::new());
        }

        // Update constraint maps
        let constraints = vx
            .parent_constraints()
            .inspect(|c| {
                // Lookup this constraint's parents
                let c_parents = self.vertex[&c.0]
                    .parent_constraints()
                    .chain(self.vertex[&c.1].parent_constraints())
                    .collect::<HashSet<_>>();

                // Is this constraint preferred?
                let prefer_conflict = self
                    .state
                    .get(&c.opposite())
                    .and_then(|s| Some(s.preferred))
                    .unwrap_or(false);
                let parents_preferred = || c_parents.iter().all(|c| self.state[c].preferred);
                let prefer = !prefer_conflict && parents_preferred();

                // Add an entry in the state map, if it doesn't already exist
                if !self.state.contains_key(c) {
                    self.state.insert(
                        *c,
                        AvalancheState::default()
                            .with_parents(c_parents)
                            .with_preference(prefer),
                    );
                }
                // Add an entry to the constraint map of every vertex constrained by these
                if let Some(map) = self.constraints.get_mut(&c.0) {
                    map.insert(*c);
                }
                if let Some(map) = self.constraints.get_mut(&c.1) {
                    map.insert(*c);
                }
            })
            .collect();
        self.constraints.insert(vx.hash(), constraints);

        // Update child map
        self.map_child(&vx.hash(), &vx.parents);

        // Register this vertex as part of the graph
        self.graph.insert(vx.hash());
    }

    /// Insert a vertex into the [`DAG`].  Returns a list of known children waiting to be inserted
    /// after this vertex
    pub fn try_insert(&mut self, vx: &Arc<Vertex>) -> Result<HashSet<VertexHash>> {
        // TODO: this should be thread safe...need mutex on DAG?
        // Check if the vertex may be inserted
        self.check_vertex(vx).map_err(|e| {
            // Add vertex as known child, even if we are missing some of its parents
            if let Error::MissingParents(_) | Error::WaitingOnParents(_) = e {
                self.map_child(&vx.hash(), &vx.parents);
                self.vertex.insert(vx.hash(), vx.clone());
            }
            e
        })?;

        // Insert the vertex into the graph
        self.insert_unchecked(vx);

        // Return any waiting children
        Ok(self
            .get_known_children(&vx.hash())?
            .into_iter()
            .filter(|vhash| !self.graph.contains(vhash))
            .collect())
    }

    /// Return true if the specified vertex is preferred over all its conflicts
    pub fn is_preferred(&self, vhash: &VertexHash) -> Result<bool> {
        let vx = self.vertex.get(vhash).ok_or(Error::NotFound)?;
        Ok(vx.parent_constraints().all(|c| self.state[&c].preferred))
    }

    /// Award a chit to the specified vertex, according to the Avalanche protocol
    pub fn award_chit(&mut self, vhash: &VertexHash) -> Result<()> {
        // Helper to recursively reset confidences. Returns true if the confidence of the specified
        // constraint has changed.
        fn reset(dag: &mut DAG, c: &Constraint) -> bool {
            // Reset self chit & confidence for this vertex
            let children_to_reset = {
                let state = dag.state.get_mut(&c).unwrap();
                let orig_conf = state.confidence;
                state.chit = false;
                state.confidence = 0;
                state.preferred = false;
                let conf_changed = orig_conf != state.confidence;
                if conf_changed {
                    Some(state.children.clone())
                } else {
                    None
                }
            };

            // Recursively reset each child
            if let Some(children) = children_to_reset {
                for child in &children {
                    reset(dag, child);
                }
                true
            } else {
                false
            }
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
                    state.confidence = usize::min(
                        dag.config.acceptance_threshold,
                        state.chit as usize + child_conf,
                    );
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

        // Award each constraint a chit, and collect which constraint states have been modified
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

    /// Get the known children of the specified [`Vertex`]
    pub fn get_known_children(&mut self, vhash: &VertexHash) -> Result<HashSet<VertexHash>> {
        self.children.get(vhash).ok_or(Error::NotFound).cloned()
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

impl AvalancheState {
    /// Assign constraint parents to the constraint state
    fn with_parents(mut self, parents: HashSet<Constraint>) -> AvalancheState {
        self.parents = parents;
        self
    }

    /// Assign preference to the constraint state
    fn with_preference(mut self, preference: bool) -> AvalancheState {
        self.preferred = preference;
        self
    }
}

#[cfg(test)]
mod test {
    use super::{Config, DAG};
    use crate::{
        consensus::dag::{self, AvalancheState},
        params,
        vertex::test_vertex,
        Vertex, WireFormat,
    };
    use std::{
        assert_matches::assert_matches,
        collections::{HashMap, HashSet},
        sync::Arc,
        thread, time,
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
        // TODO: must test case where vertex references two conflicting parents
        let gen = Arc::new(Vertex::empty());
        let v0 = test_vertex([&gen]);
        let v1 = test_vertex([&gen]);
        let v2 = test_vertex([&gen]);
        let v3 = test_vertex([&v1, &v2]);
        let mut dag = DAG::new(Config::default());

        dag.vertex.insert(gen.hash(), gen.clone());
        dag.children.insert(gen.hash(), HashSet::new()); // Genesis is "processed"
        dag.children.insert(v0.hash(), HashSet::new());
        dag.vertex.insert(v0.hash(), v0.clone()); // c0 is "processed"
        assert_matches!(dag.check_vertex(&v0), Err(dag::Error::AlreadyInserted));
        match dag.check_vertex(&v3) {
            Err(dag::Error::MissingParents(missing)) => {
                assert_eq!(missing, [v1.hash(), v2.hash()])
            }
            _ => panic!("Expected Error::MissingParents"),
        }
        dag.children.insert(v1.hash(), HashSet::new()); // c1 is known but not processed
        match dag.check_vertex(&v3) {
            Err(dag::Error::MissingParents(missing)) => {
                assert_eq!(missing, [v2.hash()])
            }
            _ => panic!("Expected Error::MissingParents"),
        }
        dag.children.insert(v2.hash(), HashSet::new()); // c2 is known but not processed
        match dag.check_vertex(&v3) {
            Err(dag::Error::WaitingOnParents(waiting)) => {
                assert_eq!(waiting, [v1.hash(), v2.hash()])
            }
            _ => panic!("Expected Error::WaitingOnParents"),
        }
        dag.children.insert(v1.hash(), HashSet::new());
        dag.vertex.insert(v1.hash(), v1.clone()); // c1 is "processed"
        match dag.check_vertex(&v3) {
            Err(dag::Error::WaitingOnParents(waiting)) => {
                assert_eq!(waiting, [v2.hash()])
            }
            _ => panic!("Expected Error::WaitingOnParents"),
        }
        dag.children.insert(v2.hash(), HashSet::new());
        dag.vertex.insert(v2.hash(), v2.clone()); // c2 is "processed"
        assert_matches!(dag.check_vertex(&v3), Ok(()));

        let c4 = test_vertex([&gen, &v1]); // Parents have mismatched height
        assert_matches!(dag.check_vertex(&c4), Err(dag::Error::BadParentHeight));

        dag.vertex.insert(v3.hash(), v3.clone());
        dag.children.insert(v3.hash(), HashSet::new()); // c3 is "processed"
        let mut c5 = Vertex::empty().with_parents([&v3]).unwrap();
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
        let v0 = test_vertex([&gen]);
        let v1 = test_vertex([&gen]);
        let v2 = test_vertex([&v0, &v1]);
        let mut dag = DAG::new(Config::default());
        dag.children.insert(gen.hash(), HashSet::new());
        dag.children.insert(v0.hash(), HashSet::new());
        dag.children.insert(v1.hash(), HashSet::new());
        dag.children.insert(v2.hash(), HashSet::new());
        assert_eq!(dag.children[&gen.hash()].len(), 0);
        assert_eq!(dag.children[&v0.hash()].len(), 0);
        assert_eq!(dag.children[&v1.hash()].len(), 0);
        assert_eq!(dag.children[&v2.hash()].len(), 0);
        dag.map_child(&v0.hash(), &v0.parents);
        assert_eq!(dag.children[&gen.hash()].len(), 1);
        assert_eq!(dag.children[&v0.hash()].len(), 0);
        assert_eq!(dag.children[&v1.hash()].len(), 0);
        assert_eq!(dag.children[&v2.hash()].len(), 0);
        dag.map_child(&v1.hash(), &v1.parents);
        assert_eq!(dag.children[&gen.hash()].len(), 2);
        assert_eq!(dag.children[&v0.hash()].len(), 0);
        assert_eq!(dag.children[&v1.hash()].len(), 0);
        assert_eq!(dag.children[&v2.hash()].len(), 0);
        dag.map_child(&v2.hash(), &v2.parents);
        assert_eq!(dag.children[&gen.hash()].len(), 2);
        assert_eq!(dag.children[&v0.hash()].len(), 1);
        assert_eq!(dag.children[&v1.hash()].len(), 1);
        assert_eq!(dag.children[&v2.hash()].len(), 0);
    }

    #[test]
    fn try_insert() {
        let gen = Arc::new(Vertex::empty());
        let v0 = test_vertex([&gen]);
        let v1 = test_vertex([&gen]);
        let v2 = test_vertex([&v0, &v1]);
        let x2 = test_vertex([&v1, &v0]); // conflicts with v2
        let v3 = test_vertex([&v2]);
        let b3 = test_vertex([&v2, &x2]); // illegal to reference conflicting parents
        let mut dag = DAG::new(Config::default());

        // Initialize graph with genesis vertex
        dag.insert_unchecked(&gen);

        // Test for missing parents, waiting parents, and preference after insertions
        match dag.try_insert(&v2) {
            Err(dag::Error::MissingParents(missing)) => {
                assert_eq!(missing, [v0.hash(), v1.hash()])
            }
            e @ _ => panic!("unexpected result: {e:?}"),
        }
        assert_eq!(dag.try_insert(&v0).unwrap(), [v2.hash()].into());
        assert!(dag.is_preferred(&v0.hash()).unwrap());
        match dag.try_insert(&v2) {
            Err(dag::Error::MissingParents(missing)) => {
                assert_eq!(missing, [v1.hash()])
            }
            e @ _ => panic!("unexpected result: {e:?}"),
        }
        match dag.try_insert(&x2) {
            Err(dag::Error::MissingParents(missing)) => {
                assert_eq!(missing, [v1.hash()])
            }
            e @ _ => panic!("unexpected result: {e:?}"),
        }
        assert_eq!(dag.try_insert(&v1).unwrap(), [v2.hash(), x2.hash(),].into());
        assert!(dag.is_preferred(&v1.hash()).unwrap());
        match dag.try_insert(&v3) {
            Err(dag::Error::WaitingOnParents(waiting)) => {
                assert_eq!(waiting, [v2.hash()])
            }
            e @ _ => panic!("unexpected result: {e:?}"),
        }
        assert_eq!(dag.try_insert(&v2).unwrap(), [v3.hash()].into());
        assert!(dag.is_preferred(&v2.hash()).unwrap());
        assert_eq!(dag.try_insert(&v3).unwrap(), [].into());
        assert!(dag.is_preferred(&v3.hash()).unwrap());
        assert_eq!(dag.try_insert(&x2).unwrap(), [].into());
        // x2 should not be preferred, due to conflict
        assert!(dag.is_preferred(&x2.hash()).unwrap() == false);
        // b3 should fail due to illegal parent combination
        assert_matches!(dag.try_insert(&b3), Err(dag::Error::ConflictingParents));
    }

    #[test]
    fn is_preferred() {
        let gen = Arc::new(Vertex::empty());
        let v0 = test_vertex([&gen]);
        let v1 = test_vertex([&gen]);
        let v2 = test_vertex([&v0, &v1]);
        let mut dag = DAG::new(Config::default());

        // Confirm that frontier is always sorted by timestamp
        assert_matches!(dag.is_preferred(&v2.hash()), Err(dag::Error::NotFound));
        dag.vertex.insert(v2.hash(), v2.clone());
        dag.constraints
            .insert(v2.hash(), v2.parent_constraints().collect());
        for constraint in v2.parent_constraints() {
            dag.state.insert(constraint, AvalancheState::default());
        }
        assert_eq!(dag.is_preferred(&v2.hash()).unwrap(), false);
        for constraint in v2.parent_constraints() {
            dag.state.get_mut(&constraint).unwrap().preferred = true;
        }
        assert_eq!(dag.is_preferred(&v2.hash()).unwrap(), true);
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
        let ten_millis = time::Duration::from_millis(10);
        let gen = Arc::new(Vertex::empty());
        thread::sleep(ten_millis);
        let v0 = test_vertex([&gen]);
        thread::sleep(ten_millis);
        let v1 = test_vertex([&gen]);
        thread::sleep(ten_millis);
        let v2 = test_vertex([&v0, &v1]);
        thread::sleep(ten_millis);
        let mut dag = DAG::new(Config::default());
        dag.vertex.insert(gen.hash(), gen.clone());
        dag.vertex.insert(v0.hash(), v0.clone());
        dag.vertex.insert(v1.hash(), v1.clone());
        dag.vertex.insert(v2.hash(), v2.clone());

        // Confirm that frontier is always sorted by timestamp
        dag.frontier.insert(v0.hash());
        dag.frontier.insert(v2.hash());
        assert_eq!(dag.get_frontier(), vec![v0.hash(), v2.hash()]);
        dag.frontier.insert(v1.hash()); // comes before c2
        assert_eq!(dag.get_frontier(), vec![v0.hash(), v1.hash(), v2.hash()]);
    }

    #[test]
    fn get_known_children() {
        let v0 = Arc::new(Vertex::empty());
        let mut dag = DAG::new(Config::default());
        assert_matches!(
            dag.get_known_children(&v0.hash()),
            Err(dag::Error::NotFound)
        );
        dag.children.insert(v0.hash(), HashSet::new());
        assert_eq!(dag.get_known_children(&v0.hash()).unwrap(), HashSet::new());
    }

    #[test]
    fn state_with_parents() {
        todo!()
    }

    #[test]
    fn state_with_preference() {
        todo!()
    }
}
