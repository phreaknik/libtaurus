use crate::{
    params,
    vertex::{self, ConflictSetKey, Constraint},
    Vertex, VertexHash, WireFormat,
};
use core::panic;
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
    #[error("mismatched data")]
    MismatchedData,
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

/// Status type indicating the current state of a [`Vertex`] in the [`DAG`]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Rejected,
    NotPreferred,
    Preferred,
    StronglyPreferred,
    Accepted,
}

/// Configuraion parameters for a [`DAG`] instance
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Hash of the graph's genesis vertex
    pub genesis: VertexHash,

    /// Counter value at which a constraint will be accepted, according to Avalanche consensus
    pub max_count: usize,

    /// Confidence value at which a constraint will be accepted under the "safe early commitment"
    /// criteria in the Avalanche specification
    pub max_confidence: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            genesis: params::DEFAULT_GENESIS_HASH,
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
            state: HashMap::new(),
            frontier: HashSet::new(),
        }
    }

    pub fn with_initial_vertices<'a, V>(config: Config, vertices: V) -> Result<DAG>
    where
        V: IntoIterator<Item = &'a Arc<Vertex>>,
    {
        let mut dag = DAG::new(config);

        // Add each vertex to the event graph, and finalize each constraint in the constraint graph
        for vx in vertices {
            dag.insert_unchecked(vx);
            dag.pending_query.remove(&vx.hash());
            for c in vx
                .parent_constraints()
                .chain(once(Constraint(vx.hash(), vx.hash())))
            {
                let state = dag.state.get_mut(&c.conflict_set_key()).unwrap();

                // Make sure none of the vertices have conflicting constraints
                if let Some(opp) = c.opposite() {
                    if state.count.get(&opp).is_some() {
                        return Err(Error::ConflictingParents);
                    }
                }

                // Set count to max value, to finalize these constraints as already accepted
                state.count = [(c, dag.config.max_count)].into();
            }
        }
        Ok(dag)
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

        // Pre-load the self-referential unity constraint for this vertex
        let parent_constraints: HashSet<_> = vx.parent_constraints().collect();
        let uc = Constraint(vx.hash(), vx.hash());
        self.state.insert(
            uc.conflict_set_key(),
            ConflictSet::new(uc, parent_constraints.clone()),
        );

        // Update constraint maps
        for c in parent_constraints.into_iter().chain(once(uc)) {
            // Lookup this constraint's parents
            let c_parents = self.vertex[&c.0]
                .parent_constraints()
                .chain(self.vertex[&c.1].parent_constraints())
                .collect::<HashSet<_>>();

            // Register constraint as child to each of its parent constraints
            for &parent in &c_parents {
                let p_state = self
                    .state
                    .get_mut(&parent.conflict_set_key())
                    .expect("parent state not found");
                if let Some(children) = p_state.children.get_mut(&parent) {
                    children.insert(c);
                } else {
                    p_state.children.insert(parent, once(c).collect());
                }
            }

            // Add an entry in the state map, if it doesn't already exist
            let _ = self
                .state
                .try_insert(c.conflict_set_key(), ConflictSet::new(c, c_parents));
        }

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
            if let Error::MissingParents(waiting) | Error::WaitingOnParents(waiting) = &e {
                self.map_child(&vx.hash(), &vx.parents);
                self.vertex.insert(vx.hash(), vx.clone());
                for vhash in waiting {
                    self.pending_query.insert(*vhash);
                }
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

    /// Record the result of an Avalanche preference query for the specified [`Vertex`]. Query
    /// result must indicate whether or not each parent constraint of the vertex was strongly
    /// preferred by a majority of peers queried.
    pub fn record_query_result(
        &mut self,
        vhash: &VertexHash,
        strongly_preferred: &[bool],
    ) -> Result<()> {
        // Helper to lookup ancestors of the given constraint, which have not yet been accepted by
        // the Avalanche consensus protocol. Warning: may contain duplicate entries.
        fn get_ancestors(dag: &mut DAG, c: &Constraint) -> Vec<Constraint> {
            // TODO: this is inefficient. Lots of allocs for each collect() and clone()
            let state = &dag.state[&c.conflict_set_key()];

            // Stop gathering ancestors once they've been accepted according to Avalanche.
            // Avalanche acceptance criteria are:
            //     counter > max count
            // or
            //     !conflict && (confidence > max_confidence) // "safe early commitment"
            if state.count[c] < dag.config.max_count
                || (c.opposite().is_none() && state.confidence[c] < dag.config.max_confidence)
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
            if vx.parent_constraints().count() != strongly_preferred.len() {
                println!(
                    ":::: {} / {}",
                    vx.parent_constraints().count(),
                    strongly_preferred.len()
                );
                Err(Error::MismatchedData)
            } else if vx.parents.iter().any(|v| self.pending_query.contains(v)) {
                Err(Error::WaitingOnParents(
                    vx.parents
                        .iter()
                        .filter(|v| self.pending_query.contains(v))
                        .copied()
                        .collect::<Vec<_>>(),
                ))
            } else if !self.graph.contains(vhash) {
                Err(Error::WaitingOnVertex)
            } else if self.pending_query.take(vhash).is_none() {
                Err(Error::AlreadyQueried)
            } else {
                // Determine each parent constraint this vertex commits to, and if preferred, award
                // a chit to each.
                let parent_constraints = vx
                    .parent_constraints()
                    .zip(strongly_preferred)
                    .inspect(|(c, &pref)| {
                        // If preferred, award each constraint a chit
                        if pref {
                            self.state
                                .get_mut(&c.conflict_set_key())
                                .unwrap()
                                .chit
                                .insert(*c, true);
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

    /// Return true if the specified [`Vertex`] is preferred over all its conflicts
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

    /// Determine the status of the specified [`Vertex`], and each constraint it commits to
    pub fn query(&self, vhash: &VertexHash) -> Result<(Status, Vec<(bool, Status)>)> {
        // Helper to get the chit and preference of a constraint
        let constraint_to_chitpref = |c: Constraint| {
            let state = self.state[&c.conflict_set_key()].clone();
            let decided = state.count[&c] >= self.config.max_count
                || (c.opposite().is_none() && state.confidence[&c] >= self.config.max_confidence);
            let preferred = state.preferred == c;
            let status = if decided && preferred {
                Status::Accepted
            } else if decided && !preferred {
                Status::Rejected
            } else if preferred {
                if state.parents[&c]
                    .clone()
                    .into_iter()
                    .all(|p| self.state[&p.conflict_set_key()].preferred == p)
                {
                    Status::StronglyPreferred
                } else {
                    Status::Preferred
                }
            } else {
                Status::NotPreferred
            };
            (state.chit[&c], status)
        };

        // Collect constraint statuses
        let mut c_stats = self
            .vertex
            .get(vhash)
            .ok_or(Error::NotFound)?
            .parent_constraints()
            .chain(once(Constraint(*vhash, *vhash))) // last constraint is self-unity
            .map(constraint_to_chitpref)
            .collect::<Vec<_>>();

        // Collect vertex statuses
        let v_stat = c_stats.iter().map(|s| s.1).min().unwrap();
        // pop last constraint, as it is not a parent constraint
        if c_stats.pop().unwrap().1 > v_stat {
            panic!("illegal state: constraint is accepted before its parents");
        }
        Ok((v_stat, c_stats))
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
    use super::{Config, Status, DAG};
    use crate::{
        consensus::dag::{self, ConflictSet},
        params,
        vertex::{make_rand_vertex, Constraint},
        Vertex, WireFormat,
    };
    use itertools::{izip, Itertools};
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
        assert_eq!(dag.graph, HashSet::new());
        assert_eq!(dag.pending_query, HashSet::new());
        assert_eq!(dag.children, HashMap::new());
        assert_eq!(dag.state, HashMap::new());
        assert_eq!(dag.frontier, HashSet::new());
    }

    #[test]
    fn new_dag_with_init() {
        let gen = Arc::new(Vertex::empty());
        let v0 = make_rand_vertex([&gen]);
        let v1 = make_rand_vertex([&gen]);
        let v2 = make_rand_vertex([&gen]);
        let v3 = make_rand_vertex([&v1, &v2]);
        let x3 = make_rand_vertex([&v2, &v1]); // conflicts with v3
        let v4 = make_rand_vertex([&v3]);

        // Should not allow vertices with conflicting parent constraints
        assert_matches!(
            DAG::with_initial_vertices(Config::default(), [&gen, &v0, &v1, &v2, &v3, &x3]),
            Err(dag::Error::ConflictingParents)
        );

        // Basic test -- genesis vertex should be accepted
        let dag = DAG::with_initial_vertices(Config::default(), [&gen]).unwrap();
        assert!(dag.pending_query.is_empty());
        assert_eq!(
            dag.query(&gen.hash()).unwrap(),
            (dag::Status::Accepted, vec![])
        );

        // Confirm each initial vertex is recognized as preferred and decided
        let mut dag = DAG::with_initial_vertices(
            {
                let mut cfg = Config::default();
                cfg.genesis = gen.hash();
                cfg
            },
            [&gen, &v0, &v1, &v2, &v3],
        )
        .unwrap();
        assert!(dag.pending_query.is_empty());
        assert_eq!(dag.query(&gen.hash()).unwrap().0, dag::Status::Accepted);
        assert_eq!(dag.query(&v0.hash()).unwrap().0, dag::Status::Accepted);
        assert_eq!(dag.query(&v1.hash()).unwrap().0, dag::Status::Accepted);
        assert_eq!(dag.query(&v2.hash()).unwrap().0, dag::Status::Accepted);
        assert_eq!(dag.query(&v3.hash()).unwrap().0, dag::Status::Accepted);

        // Check that we are able to insert a vertex which extends the initial set
        assert_matches!(dag.query(&v4.hash()), Err(dag::Error::NotFound));
        dag.try_insert(&v4).unwrap();
    }

    #[test]
    fn check_vertex() {
        let gen = Arc::new(Vertex::empty());
        let v0 = make_rand_vertex([&gen]);
        let v1 = make_rand_vertex([&gen]);
        let v2 = make_rand_vertex([&gen]);
        let v3 = make_rand_vertex([&v1, &v2]);
        let x3 = make_rand_vertex([&v2, &v1]); // conflicts with v3
        let b4 = make_rand_vertex([&v3, &x3]); // illegal! references conflicting parents
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
        let c4 = make_rand_vertex([&gen, &v1]); // Parents have mismatched height
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
        let v0 = make_rand_vertex([&gen]);
        let v1 = make_rand_vertex([&gen]);
        let v2 = make_rand_vertex([&v0, &v1]);
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
        let v0 = make_rand_vertex([&gen]);
        let v1 = make_rand_vertex([&gen]);
        let v2 = make_rand_vertex([&v0, &v1]);
        let x2 = make_rand_vertex([&v1, &v0]); // conflicts with v2
        let v3 = make_rand_vertex([&v2]);
        let b3 = make_rand_vertex([&v2, &x2]); // illegal to reference conflicting parents
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
    fn record_query_result() {
        let gen = Arc::new(Vertex::empty());
        let v0 = make_rand_vertex([&gen]);
        let v1 = make_rand_vertex([&gen]);
        let v2 = make_rand_vertex([&v0, &v1]);
        let mut dag = DAG::with_initial_vertices(Config::default(), [&gen]).unwrap();
        dag.try_insert(&v0).unwrap();

        // Not enough preferences
        assert_matches!(
            dag.record_query_result(&v0.hash(), &[]),
            Err(dag::Error::MismatchedData),
        );
        // Too many preferences
        assert_matches!(
            dag.record_query_result(&v0.hash(), &[true, true]),
            Err(dag::Error::MismatchedData),
        );
        dag.record_query_result(&v0.hash(), &[true]).unwrap();
        assert_matches!(
            dag.record_query_result(&v0.hash(), &[true]),
            Err(dag::Error::AlreadyQueried)
        );
        assert_matches!(
            dag.record_query_result(&v2.hash(), &[true, true, true]),
            Err(dag::Error::NotFound)
        );
        // will fail, but makes v2 known
        dag.try_insert(&v2).unwrap_err();
        // v1 still has not been inserted
        assert_matches!(
            dag.record_query_result(&v2.hash(), &[true, true, true]),
            Err(dag::Error::WaitingOnParents(_))
        );
        dag.try_insert(&v1).unwrap();
        // v1 still has not been queried
        assert_matches!(
            dag.record_query_result(&v2.hash(), &[true, true, true]),
            Err(dag::Error::WaitingOnParents(_))
        );
        dag.record_query_result(&v1.hash(), &[true]).unwrap();
        assert_matches!(
            dag.record_query_result(&v2.hash(), &[true, true, true]),
            Err(dag::Error::WaitingOnVertex)
        );
        dag.try_insert(&v2).unwrap();
        dag.record_query_result(&v2.hash(), &[true, true, true])
            .unwrap();
    }

    #[test]
    fn is_preferred() {
        let gen = Arc::new(Vertex::empty());
        let v0 = make_rand_vertex([&gen]);
        let v1 = make_rand_vertex([&gen]);
        let v2 = make_rand_vertex([&v0, &v1]);
        let x2 = make_rand_vertex([&v1, &v0]); // Conflicts with v2
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
    fn query() {
        // Helper to set constraint states
        fn set_state(
            dag: &mut DAG,
            c: &Constraint,
            c_state: (bool, usize, usize),
            parent_preferences: &[bool],
        ) {
            let parents = {
                let (preferred, conf, count) = c_state;
                let state = dag.state.get_mut(&c.conflict_set_key()).unwrap();
                assert!(state.parents[&c].len() == parent_preferences.len());
                if preferred {
                    state.preferred = *c;
                } else {
                    state.preferred = c.opposite().unwrap();
                }
                *state.confidence.get_mut(&c).unwrap() = conf;
                *state.count.get_mut(&c).unwrap() = count;
                state.parents[&c].clone()
            };
            for (p, &preferred) in izip!(
                parents.into_iter().sorted_by_key(|x| x.is_unity()),
                parent_preferences
            ) {
                let state = dag.state.get_mut(&p.conflict_set_key()).unwrap();
                if preferred {
                    state.preferred = p;
                } else {
                    state.preferred = p.opposite().unwrap();
                }
            }
        }

        const MAX_CONF: usize = 9;
        const MAX_COUNT: usize = 9;

        let gen = Arc::new(Vertex::empty());
        let v0 = make_rand_vertex([&gen]);
        let v1 = make_rand_vertex([&gen]);
        let v2 = make_rand_vertex([&v0, &v1]);
        let v3 = make_rand_vertex([&v0]);
        let v4 = make_rand_vertex([&v2, &v3]);
        let mut dag = DAG::with_initial_vertices(
            Config {
                genesis: gen.hash(),
                max_confidence: MAX_CONF,
                max_count: MAX_COUNT,
            },
            [&gen],
        )
        .unwrap();

        dag.try_insert(&v0).unwrap();
        dag.try_insert(&v1).unwrap();
        dag.try_insert(&v2).unwrap();
        dag.try_insert(&v3).unwrap();

        assert_matches!(dag.query(&v4.hash()), Err(dag::Error::NotFound));
        dag.try_insert(&v4).unwrap();

        set_state(
            &mut dag,
            &Constraint(v2.hash(), v3.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v2.hash(), v2.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v3.hash(), v3.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true],
        );
        assert_eq!(
            dag.query(&v4.hash()).unwrap(),
            (
                Status::StronglyPreferred,
                vec![
                    (false, Status::Accepted),
                    (false, Status::Accepted),
                    (false, Status::Accepted)
                ]
            )
        );
        set_state(
            &mut dag,
            &Constraint(v4.hash(), v4.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true, true, true],
        );
        assert_eq!(
            dag.query(&v4.hash()).unwrap(),
            (
                Status::Accepted,
                vec![
                    (false, Status::Accepted),
                    (false, Status::Accepted),
                    (false, Status::Accepted)
                ]
            )
        );

        set_state(
            &mut dag,
            &Constraint(v2.hash(), v3.hash()),
            (false, MAX_CONF, MAX_COUNT),
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v2.hash(), v2.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v3.hash(), v3.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true],
        );
        assert_eq!(
            dag.query(&v4.hash()).unwrap(),
            (
                Status::Rejected,
                vec![
                    (false, Status::Rejected),
                    (false, Status::Accepted),
                    (false, Status::Accepted)
                ]
            )
        );

        set_state(
            &mut dag,
            &Constraint(v2.hash(), v3.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v2.hash(), v2.hash()),
            (true, 0, MAX_COUNT), // still decided because of count
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v3.hash(), v3.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true],
        );
        assert_eq!(
            dag.query(&v4.hash()).unwrap(),
            (
                Status::Accepted,
                vec![
                    (false, Status::Accepted),
                    (false, Status::Accepted),
                    (false, Status::Accepted)
                ]
            )
        );

        set_state(
            &mut dag,
            &Constraint(v2.hash(), v3.hash()),
            (true, MAX_CONF, 0), // Undecided, conflict doesn't allow safe early commitment
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v2.hash(), v2.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v3.hash(), v3.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true],
        );
        assert_eq!(
            dag.query(&v4.hash()).unwrap(),
            (
                Status::StronglyPreferred,
                vec![
                    (false, Status::StronglyPreferred),
                    (false, Status::Accepted),
                    (false, Status::Accepted)
                ]
            )
        );

        set_state(
            &mut dag,
            &Constraint(v2.hash(), v3.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v2.hash(), v2.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v3.hash(), v3.hash()),
            (true, MAX_CONF, 0), // still decided for safe early commitment
            &[true],
        );
        assert_eq!(
            dag.query(&v4.hash()).unwrap(),
            (
                Status::Accepted,
                vec![
                    (false, Status::Accepted),
                    (false, Status::Accepted),
                    (false, Status::Accepted)
                ]
            )
        );

        set_state(
            &mut dag,
            &Constraint(v2.hash(), v3.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v2.hash(), v2.hash()),
            (true, 0, 0), // no longer decided
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v3.hash(), v3.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true],
        );
        assert_eq!(
            dag.query(&v4.hash()).unwrap(),
            (
                Status::StronglyPreferred,
                vec![
                    (false, Status::Accepted),
                    (false, Status::Accepted),
                    (false, Status::StronglyPreferred)
                ]
            )
        );

        set_state(
            &mut dag,
            &Constraint(v2.hash(), v3.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v2.hash(), v2.hash()),
            (true, 0, 0), // no longer decided
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v3.hash(), v3.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true],
        );
        assert_eq!(
            dag.query(&v4.hash()).unwrap(),
            (
                Status::StronglyPreferred,
                vec![
                    (false, Status::Accepted),
                    (false, Status::Accepted),
                    (false, Status::StronglyPreferred)
                ]
            )
        );

        set_state(
            &mut dag,
            &Constraint(v2.hash(), v3.hash()),
            (true, 0, 0),
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v2.hash(), v2.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[false, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v3.hash(), v3.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true],
        );
        assert_eq!(
            dag.query(&v4.hash()).unwrap(),
            (
                Status::Preferred,
                vec![
                    (false, Status::Preferred),
                    (false, Status::Accepted),
                    (false, Status::Accepted)
                ]
            )
        );

        set_state(
            &mut dag,
            &Constraint(v2.hash(), v3.hash()),
            (false, 0, 0), // not preferred or decided
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v2.hash(), v2.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true, true, true],
        );
        set_state(
            &mut dag,
            &Constraint(v3.hash(), v3.hash()),
            (true, MAX_CONF, MAX_COUNT),
            &[true],
        );
        assert_eq!(
            dag.query(&v4.hash()).unwrap(),
            (
                Status::NotPreferred,
                vec![
                    (false, Status::NotPreferred),
                    (false, Status::Accepted),
                    (false, Status::Accepted)
                ]
            )
        );
    }

    #[test]
    fn get_frontier() {
        let ten_millis = time::Duration::from_millis(10);
        let gen = Arc::new(Vertex::empty());
        thread::sleep(ten_millis);
        let v0 = make_rand_vertex([&gen]);
        thread::sleep(ten_millis);
        let v1 = make_rand_vertex([&gen]);
        thread::sleep(ten_millis);
        let v2 = make_rand_vertex([&v0, &v1]);
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
        let v10 = make_rand_vertex([&gen]);
        let v11 = make_rand_vertex([&gen]);
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
        let v0 = make_rand_vertex([&gen]);
        let v1 = make_rand_vertex([&gen]);
        let v2 = make_rand_vertex([&v0, &v1]);
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
