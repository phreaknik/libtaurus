use crate::{
    params,
    vertex::{self, ConflictSetKey, Constraint},
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
    #[error("already queried")]
    AlreadyQueried,
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
    #[error("waiting on vertex")]
    WaitingOnVertex,
    #[error("waiting on vertex parents")]
    WaitingOnParents(Vec<VertexHash>),
}
type Result<T> = result::Result<T, Error>;

/// Configuraion parameters for a [`DAG`] instance
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    // Counter value at which a constraint will be accepted, according to Avalanche consensus
    max_count: usize,

    // Confidence value at which a constraint will be accepted, according to Avalanche consensus
    max_confidence: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_count: params::AVALANCHE_COUNTER_THRESHOLD,
            max_confidence: params::AVALANCHE_CONFIDENCE_THRESHOLD,
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

    /// Set of vertices which are waiting for the results of a query
    pending_query: HashSet<VertexHash>,

    /// Map of known children for each vertex in the event graph
    children: HashMap<VertexHash, HashSet<VertexHash>>,

    /// Map of each vertex to the list of ordering constraints applied to it
    constraints: HashMap<VertexHash, HashSet<Constraint>>,

    /// Map of each ordering constraint to its corresponding Avalanche state variables
    state: HashMap<ConflictSetKey, ConflictSet>,

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
            pending_query: HashSet::new(),
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

        // Confirm parents do not have conflicting constraints
        let constraints_of_parents = vx
            .parents
            .iter()
            .map(|vhash| self.vertex[vhash].parent_constraints())
            .flatten()
            .collect::<HashSet<_>>();
        if constraints_of_parents
            .iter()
            .filter_map(|c| c.opposite())
            .any(|opp| constraints_of_parents.contains(&opp))
        {
            return Err(Error::ConflictingParents);
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

    /// Insert a [`Vertex`] without checking DAG constraints. Will panic if parents have not
    /// already been inserted.
    fn insert_unchecked(&mut self, vx: &Arc<Vertex>) {
        // Add vertex to list of inserted vertices
        self.vertex.insert(vx.hash(), vx.clone());

        // Add an entry in the child map, if it doesn't already exist
        let _ = self.children.try_insert(vx.hash(), HashSet::new());

        // Update constraint maps
        let constraints = vx
            .parent_constraints()
            .inspect(|&c| {
                // Lookup this constraint's parents
                let c_parents = self.vertex[&c.0]
                    .parent_constraints()
                    .chain(self.vertex[&c.1].parent_constraints())
                    .collect::<HashSet<_>>();

                // Register constraint as child to each of its parent constraints
                for parent in &c_parents {
                    self.state
                        .get_mut(&parent.conflict_set_key())
                        .unwrap()
                        .children
                        .get_mut(parent)
                        .unwrap()
                        .insert(c);
                }

                // Add an entry in the state map, if it doesn't already exist
                let _ = self
                    .state
                    .try_insert(c.conflict_set_key(), ConflictSet::new(c, c_parents));

                // Add an entry to the constraint map of each vertex constrained by this constraint
                if let Some(map) = self.constraints.get_mut(&c.0) {
                    map.insert(c);
                }
                if let Some(map) = self.constraints.get_mut(&c.1) {
                    map.insert(c);
                }
            })
            .collect();
        self.constraints.insert(vx.hash(), constraints);

        // Update child map
        self.map_child(&vx.hash(), &vx.parents);

        // Register this vertex as part of the graph and waiting for query results
        self.graph.insert(vx.hash());
        self.pending_query.insert(vx.hash());
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
        Ok(self
            .vertex
            .get(vhash)
            .ok_or(Error::NotFound)
            .and_then(|vx| {
                self.graph
                    .get(vhash)
                    .ok_or(Error::WaitingOnVertex)
                    .map(|_| vx)
            })?
            .parent_constraints()
            .all(|c| self.state[&c.conflict_set_key()].preferred == c))
    }

    /// Record the result of an Avalanche preference query for the specified [`Vertex`]
    pub fn record_query_result(&mut self, vhash: &VertexHash, preferred: &[bool]) -> Result<()> {
        // Helper to lookup ancestors of the given constraint, which have not yet been accepted by
        // the Avalanche consensus protocol. Warning: may contain duplicate entries.
        fn get_ancestors(dag: &mut DAG, c: &Constraint) -> Vec<Constraint> {
            // TODO: this is inefficient. Lots of allocs for each collect() and clone()
            let state = &dag.state[&c.conflict_set_key()];

            // Stop gathering ancestors once they've been accepted according to Avalanche
            if state.count[c] < dag.config.max_count
                && state.confidence[c] < dag.config.max_confidence
            {
                state.parents[c]
                    .clone()
                    .into_iter()
                    .flat_map(|p| once(p).chain(get_ancestors(dag, &p).into_iter()))
                    .collect()
            } else {
                Vec::new()
            }
        }

        // Helper to lookup every constraint which has a chit in the progeny of the given
        // constraint. Warning: may contain duplicate entries.
        fn chits_in_progeny(dag: &mut DAG, c: &Constraint) -> Vec<Constraint> {
            // TODO: this is inefficient, especially since this is called repeatedly to look up
            // constraints in the progeny which are 99% unchanged.
            let state = dag.state[&c.conflict_set_key()].clone();
            let mut set = state.children[c]
                .clone()
                .into_iter()
                .flat_map(|d| chits_in_progeny(dag, &d).into_iter())
                .collect::<Vec<_>>();
            if state.chit[c] {
                set.push(*c);
            }
            set
        }

        // Check that we have the vertex, and that we are able to record a query for it.
        if let Some(vx) = self.vertex.get(vhash).cloned() {
            if !self.graph.contains(vhash) {
                Err(Error::WaitingOnVertex)
            } else if vx.parents.iter().any(|v| self.pending_query.contains(v)) {
                Err(Error::WaitingOnParents(
                    vx.parents
                        .iter()
                        .filter(|v| self.pending_query.contains(v))
                        .copied()
                        .collect::<Vec<_>>(),
                ))
            } else if self.pending_query.take(vhash).is_none() {
                Err(Error::AlreadyQueried)
                // TODO: test this
            } else {
                // Determine each parent constraint this vertex commits to, and if preferred, award
                // a chit to each.
                let parent_constraints = vx
                    .parent_constraints()
                    .zip(preferred)
                    .inspect(|(c, &pref)| {
                        // If preferred, award each constraint a chit
                        if pref {
                            *self
                                .state
                                .get_mut(&c.conflict_set_key())
                                .unwrap()
                                .chit
                                .get_mut(&c)
                                .unwrap() = true;
                        }
                    })
                    .collect::<HashSet<_>>();

                // Update the state of each ancestor of a constraint, according to the query result
                // for that constraint
                for (c, &pref) in parent_constraints.into_iter().sorted_by_key(|x| x.1) {
                    for a in get_ancestors(self, &c).into_iter().unique() {
                        if pref {
                            // If this constraint has a conflict, compute its confidence
                            let conf_opp = a
                                .opposite()
                                .map(|opp| {
                                    chits_in_progeny(self, &opp).into_iter().unique().count()
                                })
                                .unwrap_or(0);
                            let conf_a = if conf_opp > 0 {
                                chits_in_progeny(self, &a).into_iter().unique().count()
                            } else {
                                1
                            };
                            let state = self.state.get_mut(&a.conflict_set_key()).unwrap();

                            // Set this constraint as preferred if it has the higher confidence
                            if conf_a > conf_opp {
                                state.preferred = a;
                            }

                            // Increment the counter, if this vote is the same as the previous vote
                            // in this conflict set.
                            if state.last != a {
                                state.last = a;
                                *self
                                    .state
                                    .get_mut(&a.conflict_set_key())
                                    .unwrap()
                                    .count
                                    .get_mut(&a)
                                    .unwrap() = 1;
                            } else {
                                *self
                                    .state
                                    .get_mut(&a.conflict_set_key())
                                    .unwrap()
                                    .count
                                    .get_mut(&a)
                                    .unwrap() += 1;
                            }

                            // If this vote reinforces the prior vote, increment the counter.
                            // Otherwise, reset the counter.
                        } else {
                            // Reset the counter
                            *self
                                .state
                                .get_mut(&a.conflict_set_key())
                                .unwrap()
                                .count
                                .get_mut(&a)
                                .unwrap() = 0;
                        }
                    }
                }

                Ok(())
            }
        } else {
            Err(Error::NotFound)
        }
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
#[derive(Clone, Debug, PartialEq, Eq)]
struct ConflictSet {
    /// Which, if any, constraint is preferred over this constraint
    preferred: Constraint,

    /// Which constraint was previously preferred
    last: Constraint,

    /// Chits for each constraint in the set
    chit: HashMap<Constraint, bool>,

    /// Confidence value (sum of chits in progeny) for each constraint in the set
    confidence: HashMap<Constraint, usize>,

    /// Vote counters for each constraint in the set
    count: HashMap<Constraint, usize>,

    /// Parents in the constraint graph for each constraint in the conflict set
    parents: HashMap<Constraint, HashSet<Constraint>>,

    /// Known children in the constraint graph for each constraint in the conflict set
    children: HashMap<Constraint, HashSet<Constraint>>,
}

impl ConflictSet {
    /// Assign constraint parents to the constraint state
    fn new(initial: Constraint, parents: HashSet<Constraint>) -> ConflictSet {
        ConflictSet {
            preferred: initial,
            last: initial,
            chit: once((initial, false)).collect(),
            confidence: once((initial, 0)).collect(),
            count: once((initial, 0)).collect(),
            parents: once((initial, parents)).collect(),
            children: once((initial, HashSet::new())).collect(),
        }
    }
}

#[cfg(test)]
mod test {
    use itertools::Itertools;

    use super::{Config, DAG};
    use crate::{
        consensus::dag::{self, ConflictSet},
        params,
        vertex::{test_vertex, Constraint},
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
        assert_eq!(dflt.max_count, params::AVALANCHE_COUNTER_THRESHOLD);
        assert_eq!(dflt.max_confidence, params::AVALANCHE_CONFIDENCE_THRESHOLD);
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
        let v0 = test_vertex([&gen]);
        let v1 = test_vertex([&gen]);
        let v2 = test_vertex([&gen]);
        let v3 = test_vertex([&v1, &v2]);
        let x3 = test_vertex([&v2, &v1]); // conflicts with v3
        let b4 = test_vertex([&v3, &x3]); // illegal! references conflicting parents
        let mut dag = DAG::new(Config::default());

        // Test for missing/waiting parents
        dag.insert_unchecked(&gen);
        dag.insert_unchecked(&v0);
        assert_matches!(dag.check_vertex(&v0), Err(dag::Error::AlreadyInserted));
        match dag.check_vertex(&v3) {
            Err(dag::Error::MissingParents(missing)) => {
                assert_eq!(missing, [v1.hash(), v2.hash()])
            }
            _ => panic!("Expected Error::MissingParents"),
        }
        dag.vertex.insert(v1.hash(), v1.clone()); // v1 is known but not processed
        match dag.check_vertex(&v3) {
            Err(dag::Error::MissingParents(missing)) => {
                assert_eq!(missing, [v2.hash()])
            }
            _ => panic!("Expected Error::MissingParents"),
        }
        dag.vertex.insert(v2.hash(), v2.clone()); // v2 is known but not processed
        match dag.check_vertex(&v3) {
            Err(dag::Error::WaitingOnParents(waiting)) => {
                assert_eq!(waiting, [v1.hash(), v2.hash()])
            }
            _ => panic!("Expected Error::WaitingOnParents"),
        }
        dag.insert_unchecked(&v1); // v1 is "processed"
        match dag.check_vertex(&v3) {
            Err(dag::Error::WaitingOnParents(waiting)) => {
                assert_eq!(waiting, [v2.hash()])
            }
            _ => panic!("Expected Error::WaitingOnParents"),
        }
        dag.insert_unchecked(&v2); // v2 is "processed"
        assert_matches!(dag.check_vertex(&v3), Ok(()));

        // Test parent heights
        let c4 = test_vertex([&gen, &v1]); // Parents have mismatched height
        assert_matches!(dag.check_vertex(&c4), Err(dag::Error::BadParentHeight));
        dag.insert_unchecked(&v3); // v3 is "processed"
        let mut c5 = Vertex::empty().with_parents([&v3]).unwrap();
        c5.height = 2; // Should have height 3
        match dag.check_vertex(&Arc::new(c5)) {
            Err(dag::Error::BadHeight(actual, expected)) => {
                assert_eq!(actual, 2);
                assert_eq!(expected, 3);
            }
            _ => panic!("Expected Error::BadHeight"),
        }

        // Catch conflicting parents
        dag.insert_unchecked(&x3); // x3 is "processed"
        assert_matches!(dag.check_vertex(&b4), Err(dag::Error::ConflictingParents));
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
        let x2 = test_vertex([&v1, &v0]); // Conflicts with v2
        let mut dag = DAG::new(Config::default());

        assert_matches!(dag.is_preferred(&v2.hash()), Err(dag::Error::NotFound));
        let _ = dag.try_insert(&v2); // V2 is known now, but still waiting on parents to process
        assert_matches!(
            dag.is_preferred(&v2.hash()),
            Err(dag::Error::WaitingOnVertex)
        );

        // Insert all parents of v2 and x2, so they will insert successfully now
        dag.insert_unchecked(&gen);
        dag.insert_unchecked(&v0);
        dag.insert_unchecked(&v1);

        dag.insert_unchecked(&v2);
        assert_matches!(dag.is_preferred(&v2.hash()), Ok(true));
        dag.insert_unchecked(&x2);
        assert_matches!(dag.is_preferred(&x2.hash()), Ok(false));
    }

    #[test]
    fn record_query_result() {
        todo!()
        /*
                // Set up test vertices and dag
                let gen = Arc::new(Vertex::empty());
                let a00 = test_vertex([&gen]);
                let a01 = test_vertex([&gen]);
                let a02 = test_vertex([&gen]);

                // Create conflicting sets b & c
                let b10 = test_vertex([&a00, &a01]);
                let b11 = test_vertex([&a00, &a02]);
                let b20 = test_vertex([&b10]);
                let b21 = test_vertex([&b10, &b11]);
                let b30 = test_vertex([&b21, &b20]);
                let c10 = test_vertex([&a01, &a00]); // Conflicts with b10
                let c11 = test_vertex([&a02, &a00]); // Conflicts with b11
                let c20 = test_vertex([&c10]);
                let c21 = test_vertex([&c10, &c11]);
                let c30 = test_vertex([&c21, &c20]);

                println!(":::: gen.hash() = {}", gen.hash());
                println!(":::: a00.hash() = {}", a00.hash());
                println!(":::: a01.hash() = {}", a01.hash());
                println!(":::: a02.hash() = {}", a02.hash());
                println!(":::: b10.hash() = {}", b10.hash());
                println!(":::: b11.hash() = {}", b11.hash());
                println!(":::: b20.hash() = {}", b20.hash());
                println!(":::: b21.hash() = {}", b21.hash());
                println!(":::: b30.hash() = {}", b30.hash());
                println!(":::: c10.hash() = {}", c10.hash());
                println!(":::: c11.hash() = {}", c11.hash());
                println!(":::: c20.hash() = {}", c20.hash());
                println!(":::: c21.hash() = {}", c21.hash());
                println!(":::: c30.hash() = {}", c30.hash());

                // Insert everything into the DAG
                const MAX_CONF: usize = 9;
                let mut dag = DAG::new(Config {
                    acceptance_threshold: MAX_CONF,
                });
                dag.insert_unchecked(&gen);
                dag.try_insert(&a00).unwrap();
                dag.try_insert(&a01).unwrap();
                dag.try_insert(&a02).unwrap();
                dag.try_insert(&b10).unwrap();
                dag.try_insert(&b11).unwrap();
                dag.try_insert(&b20).unwrap();
                dag.try_insert(&b21).unwrap();
                dag.try_insert(&b30).unwrap();
                dag.try_insert(&c10).unwrap();
                dag.try_insert(&c11).unwrap();
                dag.try_insert(&c20).unwrap();
                dag.try_insert(&c21).unwrap();
                dag.try_insert(&c30).unwrap();

                // Test preferences based on insertion without chits
                assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), false);

                // Award chits to gen & "a" vertices, and confirm preferences don't change
                dag.record_query_result(&gen.hash()).unwrap();
                dag.record_query_result(&a00.hash()).unwrap();
                dag.record_query_result(&a01.hash()).unwrap();
                dag.record_query_result(&a02.hash()).unwrap();
                assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), false);

                // Award chit to c10 and confirm "c" subtree becomes partially preferred
                dag.record_query_result(&c10.hash()).unwrap();
                assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), true); // Does not conflict with c10
                assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), false);

                // Award chit to c11 to get full "c" subtree preference
                dag.record_query_result(&c11.hash()).unwrap();
                assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), true);

                // Award chits to rest of "c" sub tree
                dag.record_query_result(&c20.hash()).unwrap();
                dag.record_query_result(&c21.hash()).unwrap();
                dag.record_query_result(&c30.hash()).unwrap();
                assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), true);

                // Award chits to "b" sub tree, to equal "c" confidence. "c" sub tree should remain
                // preferred
                dag.record_query_result(&b10.hash()).unwrap();
                assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), true);
                dag.record_query_result(&b11.hash()).unwrap();
                assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), true);
                dag.record_query_result(&b20.hash()).unwrap();
                assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), true);
                dag.record_query_result(&b21.hash()).unwrap();
                assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), true);
                dag.record_query_result(&b30.hash()).unwrap();
                assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), true);

                // Extend "b" subtree once more. Should be enough to flip "b" subtree into preference.
                let c40 = test_vertex([&c30]);
                dag.try_insert(&c40).unwrap();
                dag.record_query_result(&c40.hash()).unwrap();
                assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), true);
                assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), false);
                assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), false);
        */
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
        dag.frontier.insert(v1.hash()); // comes before v2
        assert_eq!(dag.get_frontier(), vec![v0.hash(), v1.hash(), v2.hash()]);
    }

    #[test]
    fn get_known_children() {
        let gen = Arc::new(Vertex::empty());
        let v10 = test_vertex([&gen]);
        let v11 = test_vertex([&gen]);
        let mut dag = DAG::new(Config::default());
        assert_matches!(
            dag.get_known_children(&gen.hash()),
            Err(dag::Error::NotFound)
        );
        dag.insert_unchecked(&gen);
        assert_eq!(dag.get_known_children(&gen.hash()).unwrap(), HashSet::new());
        dag.insert_unchecked(&v10);
        assert!(dag
            .get_known_children(&gen.hash())
            .unwrap()
            .into_iter()
            .eq([v10.hash()]));
        dag.insert_unchecked(&v11);
        assert!(dag
            .get_known_children(&gen.hash())
            .unwrap()
            .into_iter()
            .sorted()
            .eq([v10.hash(), v11.hash()].into_iter().sorted()));
    }

    #[test]
    fn new_state() {
        let gen = Arc::new(Vertex::empty());
        let v0 = test_vertex([&gen]);
        let v1 = test_vertex([&gen]);
        let v2 = test_vertex([&v0, &v1]);
        let constraint = Constraint(v2.hash(), v2.hash()); // Unity contraint on v2
        let c_parents = v2.parent_constraints().collect::<HashSet<_>>();
        let cs = ConflictSet::new(constraint, c_parents.clone());
        assert_eq!(cs.preferred, constraint);
        assert_eq!(cs.last, constraint);
        assert!(cs.chit.into_iter().eq([(constraint, false)]));
        assert!(cs.confidence.into_iter().eq([(constraint, 0)]));
        assert!(cs.count.into_iter().eq([(constraint, 0)]));
        assert_eq!(cs.parents.len(), 1);
        assert!(cs.parents[&constraint].iter().eq(&c_parents));
        assert!(cs.children.into_iter().eq([(constraint, HashSet::new())]));
    }
}
