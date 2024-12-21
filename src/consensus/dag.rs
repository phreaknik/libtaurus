use crate::{
    vertex::{self, ConflictSetKey, Constraint},
    Vertex, VertexHash, WireFormat,
};
use itertools::Itertools;
use rayon::prelude::*;
use std::{
    cmp,
    collections::{HashMap, HashSet},
    iter::once,
    result,
    sync::Arc,
};
use tracing::{debug, error, warn};

const DFLT_ACCEPTANCE_THRESHOLD: usize = 9;
const DFLT_SAFE_EARLY_COMMIT_THRESHOLD: usize = 6;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("already inserted")]
    AlreadyInserted,
    #[error("already recorded")]
    AlreadyRecorded,
    #[error("config specifies invalid decision thresholds")]
    BadCfgThreshold,
    #[error("bad height (expected {0}, found {1})")]
    BadHeight(u64, u64),
    #[error("ancestors conflict")]
    ConflictingAncestors,
    #[error("missing parents")]
    MissingParents(Vec<VertexHash>),
    #[error("not found")]
    NotFound,
    #[error("ancestor has been rejected")]
    RejectedAncestor,
    #[error("self referential parent")]
    SelfReferentialParent,
    #[error(transparent)]
    Vertex(#[from] vertex::Error),
    #[error("waiting on vertex parents")]
    WaitingOnParents(Vec<VertexHash>),
    #[error("waiting on vertex to be inserted")]
    WaitingOnInsert,
}
type Result<T> = result::Result<T, Error>;

/// Configuraion parameters for a [`DAG`] instance
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Hash of the graph's genesis vertex
    pub genesis: VertexHash,

    /// Counter value at which a constraint will be accepted, according to Avalanche consensus
    pub thresh_accepted: usize,

    /// Counter value at which a constraint will be accepted, according to the Avalanche "safe
    /// early committment" criteria
    pub thresh_safe_early_commit: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            genesis: VertexHash::default(),
            thresh_accepted: DFLT_ACCEPTANCE_THRESHOLD,
            thresh_safe_early_commit: DFLT_SAFE_EARLY_COMMIT_THRESHOLD,
        }
    }
}

impl Config {
    /// Check the configuration parameters are legal
    pub fn check(&self) -> Result<()> {
        if self.thresh_accepted < self.thresh_safe_early_commit {
            Err(Error::BadCfgThreshold)
        } else {
            Ok(())
        }
    }
}

/// Implementation of SESAME DAG. Each [`Vertex`] represents a batch of transactions in the graph.
/// Each [`Vertex`] commits to a set of ordering [`Constraint`]s for each combination of parent
/// vertices. We arrange these ordering constraints into an independent constraint graph, and use
/// Avalanche consensus to reach agreement on the total ordering of vertices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DAG {
    /// Configuration parameters
    config: Config,

    /// Map of every known undecided vertex
    vertex: HashMap<VertexHash, Arc<Vertex>>,

    /// Set of vertices which are in the active region of the graph
    graph: HashMap<VertexHash, Arc<Vertex>>,

    /// Set of vertices which are waiting for the results of a query
    pending_query: HashMap<VertexHash, Arc<Vertex>>,

    /// Map of known children for each vertex in the graph
    children: HashMap<VertexHash, HashSet<VertexHash>>,

    /// Map of each ordering constraint to its corresponding Avalanche state variables
    state: HashMap<ConflictSetKey, ConflictSet>,

    /// A collection of all vertices without children. This list may include vertices which are not
    /// strongly preferred.
    frontier: HashMap<VertexHash, Arc<Vertex>>,
}

impl DAG {
    /// Create a new [`DAG`] with the given [`Config`] and initial decided vertices
    pub fn new<'a, V>(config: Config, init: V) -> Result<DAG>
    where
        V: IntoIterator<Item = &'a Arc<Vertex>>,
    {
        // Check the config
        config.check()?;

        // Create a new dag instance
        let mut dag = DAG {
            config,
            vertex: HashMap::new(),
            graph: HashMap::new(),
            pending_query: HashMap::new(),
            children: HashMap::new(),
            state: HashMap::new(),
            frontier: HashMap::new(),
        };

        // Add each vertex to the graph, and finalize each constraint in the constraint graph
        for vx in init {
            dag.insert(vx);
            dag.pending_query.remove(&vx.hash());
            for c in vx
                .parent_constraints()
                .chain(once(Constraint(vx.hash(), vx.hash())))
            {
                // Mark the associated constraints as accepted
                dag.state
                    .get_mut(&c.conflict_set_key())
                    .unwrap()
                    .decision
                    .insert(c, true);
            }
        }
        Ok(dag)
    }

    // Get the list of ancestors back to the point where all ancestors have been accepted. Results
    // will be sorted according to ascending height.
    fn get_active_ancestors(&self, vx: &Vertex) -> Result<Vec<VertexHash>> {
        // Helper function to find the maximum height at which all ancestors have been decided.
        fn decided_height(dag: &DAG, vhash: &VertexHash) -> Result<u64> {
            let unity = Constraint(*vhash, *vhash);
            let state = &dag.state[&unity.conflict_set_key()];
            match state.decision.get(&unity) {
                // Vertex must not map to a rejected ancestor
                Some(false) => Err(Error::RejectedAncestor),

                // Stop recursing once we find an accepted ancestor at this branch
                Some(true) => Ok(dag.vertex[vhash].height),

                // Otherwise, branch out to each parent and find the minimum decided_height of
                // their ancestors
                None => dag.vertex[vhash]
                    .parents
                    .par_iter() // TODO: make Rayon parallelism optional
                    .map(|p| decided_height(dag, p))
                    .try_reduce(|| u64::MAX, |a, b| Ok(cmp::min(a, b))),
            }
        }

        // Helper function to gather ancestors of the given vertex back to the specified height.
        fn gather_ancestors(dag: &DAG, vhash: &VertexHash, to_height: u64) -> HashSet<VertexHash> {
            let vx = &dag.vertex[vhash];
            if vx.height > to_height {
                vx.parents
                    .iter()
                    .copied()
                    .chain(
                        vx.parents
                            .iter()
                            .flat_map(|p| gather_ancestors(dag, &p, to_height).into_iter()),
                    )
                    .collect()
            } else {
                HashSet::new()
            }
        }

        if vx.parents.iter().any(|p| !self.vertex.contains_key(p)) {
            return Err(Error::NotFound);
        }

        let min_heights: Vec<_> = vx
            .parents
            .iter()
            .map(|p| decided_height(self, p))
            .try_collect()?;
        let to_height = itertools::min(min_heights).unwrap();
        let ancestors_of_parents: HashSet<_> = vx
            .parents
            .iter()
            .flat_map(|p| gather_ancestors(self, p, to_height).into_iter())
            .collect();
        if vx.parents.iter().any(|p| ancestors_of_parents.contains(p)) {
            Err(Error::SelfReferentialParent)
        } else {
            Ok(ancestors_of_parents
                .into_iter()
                .chain(vx.parents.iter().copied())
                .filter(|&vhash| {
                    // TODO: this kills performance
                    let unity = Constraint(vhash, vhash);
                    !self.state[&unity.conflict_set_key()]
                        .decision
                        .contains_key(&unity)
                })
                .sorted_by(|a, b| Ord::cmp(&self.vertex[a].height, &self.vertex[b].height))
                .collect())
        }
    }

    // Get the list of constraints for every ancestor back to the point where all ancestors have
    // been accepted. Results will be sorted according to ascending height.
    fn get_active_ancestor_constraints(&self, vx: &Vertex) -> Result<Vec<Constraint>> {
        Ok(self
            .get_active_ancestors(vx)?
            .iter()
            .flat_map(|vhash| self.vertex[vhash].parent_constraints())
            .chain(vx.parent_constraints())
            .unique() // TODO: this eats performance
            .filter(|c| match self.state.get(&c.conflict_set_key()) {
                Some(s) => !s.decision.contains_key(&c),
                None => true,
            }) // TODO: perf
            .collect::<Vec<_>>())
    }

    /// Before inserting a vertex into the [`DAG`] it must pass these checks. Will collect
    /// ancestors (vertices and constraints), confirm that this vertex is a legal extension of the
    /// graph, and return the ancestor sets if so.
    fn check_vertex(&self, vx: &Vertex) -> Result<()> {
        // Rule out any obviously insane vertices
        vx.sanity_checks()?;

        // Check the child map to make sure this vertex doesn't already exist
        if self.graph.contains_key(&vx.hash()) {
            return Err(Error::AlreadyInserted);
        }

        // Collect any parent vertices which we do not have yet, or which we are still waiting to
        // be processed
        let mut missing = Vec::with_capacity(vx.parents.len());
        let mut waiting = Vec::with_capacity(vx.parents.len());
        for vhash in &vx.parents {
            if !self.vertex.contains_key(vhash) {
                missing.push(*vhash);
            } else if !self.graph.contains_key(vhash) {
                waiting.push(*vhash);
            }
        }
        if !missing.is_empty() {
            return Err(Error::MissingParents(missing));
        }
        if !waiting.is_empty() {
            return Err(Error::WaitingOnParents(waiting));
        }

        // Confirm vertex height is 1 + parent height
        let expected_height = 1 + vx
            .parents
            .iter()
            .map(|p| self.vertex[p].height)
            .max()
            .unwrap();
        if expected_height != vx.height {
            return Err(Error::BadHeight(expected_height, vx.height));
        }

        // Search for constraint conflicts in the ancestry
        let ancestor_constraints = self.get_active_ancestor_constraints(vx)?;
        for c in &ancestor_constraints {
            if let Some(cflct) = c.conflict() {
                if ancestor_constraints.contains(&cflct) {
                    return Err(Error::ConflictingAncestors);
                }
            }
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
    fn insert(&mut self, vx: &Arc<Vertex>) {
        // Add vertex to list of inserted vertices
        self.vertex.insert(vx.hash(), vx.clone());

        // Add an entry in the child map, if it doesn't already exist
        let _ = self.children.try_insert(vx.hash(), HashSet::new());

        // Pre-load the self-referential unity constraint for this vertex
        let parent_constraints: HashSet<_> = vx.parent_constraints().collect();
        let unity = Constraint(vx.hash(), vx.hash());

        // Update constraint maps
        for c in parent_constraints.into_iter().chain(once(unity)) {
            // Lookup this constraint's parents
            let c_parents = self.vertex[&c.0]
                .parent_constraints()
                .chain(self.vertex[&c.1].parent_constraints())
                .collect::<HashSet<_>>();

            // If any of the parents are rejected, this vertex is automatically rejected
            let rejected = c_parents
                .iter()
                .any(|p| self.state[&p.conflict_set_key()].decision.get(p) == Some(&false));

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
            let mut new_state = ConflictSet::new(c, c_parents);
            if rejected {
                new_state.decision.insert(c, false);
            }
            let _ = self.state.try_insert(c.conflict_set_key(), new_state);
        }

        // Update child map
        self.map_child(&vx.hash(), &vx.parents);

        // Register this vertex as part of the graph and waiting for query results
        self.graph.insert(vx.hash(), vx.clone());
        self.pending_query.insert(vx.hash(), vx.clone());

        // Update the frontier
        for parent in &vx.parents {
            self.frontier.remove(parent);
        }
        self.frontier.insert(vx.hash(), vx.clone());
    }

    /// Retry inserting the specified pending vertices, as well as any pending vertices which
    /// depended on these.
    pub fn retry_pending<'a, H>(&mut self, hashes: H) -> Result<HashSet<VertexHash>>
    where
        H: IntoIterator<Item = &'a VertexHash>,
    {
        let mut successes = HashSet::new();
        let mut queue: Vec<VertexHash> = hashes.into_iter().copied().collect();
        while let Some(vhash) = queue.pop() {
            if let Some(vx) = self.vertex.get(&vhash).cloned() {
                if let Ok(waiting) = self.try_insert(&vx) {
                    successes.insert(vhash);
                    for w in waiting {
                        queue.push(w);
                    }
                }
            }
        }
        Ok(successes)
    }

    /// Insert a vertex into the [`DAG`].  Returns a list of known children waiting to be inserted
    /// after this vertex
    pub fn try_insert(&mut self, vx: &Arc<Vertex>) -> Result<HashSet<VertexHash>> {
        // TODO: this should be thread safe...need mutex on DAG?
        // Check if the vertex may be inserted
        self.check_vertex(vx).map_err(|e| {
            // Add vertex as known child, even if we are missing some of its parents
            if let Error::MissingParents(_) | Error::WaitingOnParents(_) = &e {
                self.map_child(&vx.hash(), &vx.parents);
                self.vertex.insert(vx.hash(), vx.clone());
            }
            e
        })?;

        // Insert the vertex into the graph
        self.insert(vx);

        // Return any waiting children
        Ok(self
            .get_known_children(&vx.hash())?
            .into_iter()
            .filter(|vhash| !self.graph.contains_key(vhash))
            .collect())
    }

    /// Record the result of an Avalanche preference query for the specified [`Vertex`]. Query
    /// result must indicate whether or not each parent constraint of the vertex was strongly
    /// preferred by a majority of peers queried.
    ///
    /// Every vertex is represented by a unity constraint on its own ordering. Thus, recording a
    /// qurey for a vertex is equivalent to recording a query for its unity constraint.
    pub fn record_query_result(
        &mut self,
        vhash: &VertexHash,
        strongly_preferred: bool,
    ) -> Result<()> {
        if strongly_preferred {
            debug!("peers prefer {vhash}");
        } else {
            warn!("peers do not prefer {vhash}");
        }

        // Helper to lookup every constraint which has a chit in the progeny of the given
        // constraint. Warning: may contain duplicate entries.
        fn progeny_with_chits(dag: &mut DAG, c: &Constraint) -> Vec<Constraint> {
            // TODO: this is inefficient, especially since this is called repeatedly to look up
            // constraints in the progeny which are 99% unchanged.
            let state = dag.state[&c.conflict_set_key()].clone();
            let mut set = state.children[c]
                .clone()
                .into_iter()
                .flat_map(|d| progeny_with_chits(dag, &d).into_iter())
                .collect::<Vec<_>>();
            if state.chit[c] {
                set.push(*c);
            }
            set
        }

        // TODO: recording preference should recursively count towards
        // ancestors which have not yet been recorded

        // Helper to mark a constraint and its progeny as rejected
        fn reject_progeny(dag: &mut DAG, c: &Constraint) {
            let children = {
                let c_state = dag.state.get_mut(&c.conflict_set_key()).unwrap();
                c_state.decision.insert(*c, false);
                c_state.children[&c].clone()
            };
            for child in children {
                reject_progeny(dag, &child);
            }
        }

        // Check that we have the vertex, and that we are able to record a query for it.
        if let Some(vx) = self.pending_query.remove(vhash) {
            // Award a chit to the unity constraint representing this vertex
            let unity = Constraint(*vhash, *vhash);
            if strongly_preferred {
                self.state
                    .get_mut(&unity.conflict_set_key())
                    .expect(&format!(
                        "state does not exist for unity constraint of {vhash}"
                    ))
                    .chit
                    .insert(unity, true);
            }

            // Update the state of all undecided constraints in the ancestry
            for c in self
                .get_active_ancestor_constraints(&vx)
                .unwrap()
                .into_iter()
                .chain(once(unity))
            {
                if strongly_preferred {
                    // Compute the confidence of this constraint and its conflict (if any)
                    let c_conf = progeny_with_chits(self, &c).into_iter().unique().count();
                    let has_conflict = c
                        .conflict()
                        .map(|opp| self.state.contains_key(&opp.conflict_set_key()))
                        .unwrap_or(false);
                    let opp_conf = c
                        .conflict()
                        .map(|opp| progeny_with_chits(self, &opp).into_iter().unique().count())
                        .unwrap_or(0);

                    // Update state variables
                    let (c_count, c_parents) = {
                        let c_state = self.state.get_mut(&c.conflict_set_key()).unwrap();

                        // If any parent constraints were rejected, yet our peers prefer this
                        // vertex, then we are in permanent conflict with our peers. i.e.
                        // hard-fork. Check that no ancester constraint has been rejected.
                        if let Some(false) = c_state.decision.get(&c) {
                            error!("hardfork detected!");
                            error!("we rejected constraint {c}, but our peers prefer it");
                        }

                        // Update the confidence score
                        c_state.confidence.insert(c, c_conf);

                        // Set this constraint as preferred if it has the higher confidence
                        if c_conf > opp_conf {
                            c_state.preferred = c;
                        }

                        // Increment the counter, if this vote is the same as the previous vote
                        // in this conflict set.
                        if c_state.last != c {
                            c_state.last = c;
                            c_state.count.insert(c, 1);
                        } else {
                            *c_state.count.get_mut(&c).unwrap() += 1;
                        }

                        (c_state.count[&c], c_state.parents[&c].clone())
                    };

                    // See if a decision can be made. A decision can be made if the vote count
                    // reaches the acceptance threshold, or if the "safe early committment"
                    // criteria are met: parents accepted, and no conflicts, and the count
                    // reaches the safe early committment threshold
                    if c_count >= self.config.thresh_accepted
                        || (c_parents.iter().all(|p| {
                            self.state[&p.conflict_set_key()].decision.get(p) == Some(&true)
                        }) && !has_conflict
                            && c_count >= self.config.thresh_safe_early_commit)
                    {
                        let c_state = self.state.get_mut(&c.conflict_set_key()).unwrap();
                        c_state.decision.insert(c, true); // accepted the constraint
                        c_state.preferred = c;
                        if let Some(opp) = c.conflict() {
                            reject_progeny(self, &opp);
                        }
                    }
                } else {
                    // If not strongly preferred, reset the counters for the entire ancestry
                    self.state
                        .get_mut(&c.conflict_set_key())
                        .expect(&format!("state does not exist for ancestor constraint {c}"))
                        .count
                        .insert(c, 0);
                }
            }

            Ok(())
        } else if self.graph.contains_key(vhash) {
            Err(Error::AlreadyRecorded)
        } else if self.vertex.contains_key(vhash) {
            Err(Error::WaitingOnInsert)
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
                    .ok_or(Error::WaitingOnInsert)
                    .map(|_| vx)
            })?
            .parent_constraints()
            .all(|c| self.state[&c.conflict_set_key()].preferred == c))
    }

    /// Queries the [`DAG`] to determine the current status of the specified [`Vertex`]. Returns
    /// two boolean values, indicating if the vertex is strongly preferred and if that decision is
    /// final (i.e. has it been accepted or rejected).
    ///
    /// For example, here is the truth table of query results:
    ///   result         | meaning
    ///   --------------------------
    ///   (false, false) | not preferred, not decided
    ///   (false, true)  | not preferred, decided (rejected)
    ///   (true, false)  | strongly-preferred, not decided
    ///   (true, true)   | strongly-preferred, decided (accepted)
    pub fn query(&self, vhash: &VertexHash) -> Result<(bool, bool)> {
        if let Some(vx) = self.graph.get(vhash) {
            // Check if there has been a decision on the unity constraint corresponding to this
            // vertex. If not, look up all ancestors and confirm their preference.
            let unity = Constraint(*vhash, *vhash);
            if let Some(decision) = self.state[&unity.conflict_set_key()].decision.get(&unity) {
                Ok(match decision {
                    true => (true, true),   // accepted
                    false => (false, true), // rejected
                })
            } else {
                let states = self
                    .get_active_ancestor_constraints(vx)?
                    .into_iter()
                    .map(|c| {
                        let c_state = self.state.get(&c.conflict_set_key()).unwrap();
                        match c_state.decision.get(&c) {
                            Some(true) => (true, true),   // accepted
                            Some(false) => (false, true), // rejected
                            None => (c_state.preferred == c, false),
                        }
                    })
                    .inspect(|s| {
                        // It should be impossible for a rejected ancestor constraint to make it
                        // here if the queried vertex is not rejected
                        assert!(
                            s.0 || !s.1, // only assert if rejected
                            "found rejected vertex in ancestry"
                        );
                    })
                    .collect::<Vec<_>>();
                Ok((states.into_iter().all(|s| s.0), false))
            }
        } else if self.vertex.contains_key(vhash) {
            Err(Error::WaitingOnInsert)
        } else {
            Err(Error::NotFound)
        }
    }

    /// Lookup the specified [`Vertex`]
    pub fn get_vertex(&mut self, vhash: &VertexHash) -> Option<Arc<Vertex>> {
        self.vertex.get(vhash).cloned()
    }

    /// Get the latest vertices which have no children, in the order we've observed them. If
    /// frontier vertices exist and different heights, will only return those at the lowest height;
    /// i.e. those which need children in order to stay attached to the [`DAG`].
    // TODO: clean up these docs ^
    pub fn get_frontier(&mut self) -> Vec<Arc<Vertex>> {
        // First, find every vertex in the DAG which is strongly preferred
        self.frontier
            .iter()
            .filter(|(vhash, _vx)| {
                self.query(vhash)
                    .ok()
                    .map(|(preferred, _)| preferred)
                    .unwrap_or(false)
            })
            .map(|(_, vx)| vx)
            .cloned()
            .sorted_by(|a, b| Ord::cmp(&a.timestamp, &b.timestamp))
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

    /// If a decision hash been made for the associated constraints (accepted or rejected), that
    /// decision will be recorded here for faster lookups
    decision: HashMap<Constraint, bool>,
}

impl ConflictSet {
    /// Assign constraint parents to the constraint state
    fn new(initial: Constraint, parents: HashSet<Constraint>) -> ConflictSet {
        let keys = match initial.conflict() {
            None => vec![initial],
            Some(opposite) => vec![initial, opposite],
        };
        ConflictSet {
            preferred: initial,
            last: initial,
            chit: keys.iter().copied().zip([false, false]).collect(),
            confidence: keys.iter().copied().zip([0, 0]).collect(),
            count: keys.iter().copied().zip([0, 0]).collect(),
            parents: keys
                .iter()
                .copied()
                .zip([parents, HashSet::new()])
                .collect(),
            children: keys
                .iter()
                .copied()
                .zip([HashSet::new(), HashSet::new()])
                .collect(),
            decision: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::{Config, DAG};
    use crate::{
        consensus::dag::{self, ConflictSet},
        vertex::{make_rand_vertex, Constraint},
        Vertex, WireFormat,
    };
    use itertools::Itertools;
    use std::{assert_matches::assert_matches, collections::HashSet, sync::Arc};

    #[test]
    fn new_dag() {
        let gen = Arc::new(Vertex::empty());
        let v0 = make_rand_vertex([&gen]);
        let v1 = make_rand_vertex([&gen]);
        let v2 = make_rand_vertex([&gen]);
        let v3 = make_rand_vertex([&v1, &v2]);
        let v4 = make_rand_vertex([&v3]);

        assert_matches!(
            DAG::new(
                Config {
                    thresh_safe_early_commit: 5,
                    thresh_accepted: 4,
                    ..Config::default()
                },
                [&gen],
            ),
            Err(dag::Error::BadCfgThreshold)
        );

        // Basic test -- genesis vertex should be accepted
        let dag = DAG::new(Config::default(), [&gen]).unwrap();
        assert!(dag.pending_query.is_empty());
        assert_eq!(dag.query(&gen.hash()).unwrap(), (true, true));

        // Confirm each initial vertex is recognized as preferred and decided
        let mut dag = DAG::new(
            {
                let mut cfg = Config::default();
                cfg.genesis = gen.hash();
                cfg
            },
            [&gen, &v0, &v1, &v2, &v3],
        )
        .unwrap();
        assert!(dag.pending_query.is_empty());
        assert_matches!(dag.query(&gen.hash()), Ok((true, true)));
        assert_matches!(dag.query(&v0.hash()), Ok((true, true)));
        assert_matches!(dag.query(&v1.hash()), Ok((true, true)));
        assert_matches!(dag.query(&v2.hash()), Ok((true, true)));
        assert_matches!(dag.query(&v3.hash()), Ok((true, true)));

        // Check that we are able to insert a vertex which extends the initial set
        assert_matches!(dag.query(&v4.hash()), Err(dag::Error::NotFound));
        dag.try_insert(&v4).unwrap();
    }

    #[test]
    fn get_active_ancestors() {
        fn sort_range<T: Ord + Copy>(v: &mut Vec<T>, range: std::ops::Range<usize>) {
            v.splice(range.clone(), {
                let mut tmp = v[range].to_vec();
                tmp.sort();
                tmp
            });
        }

        let gen = Arc::new(Vertex::empty());
        let v00 = make_rand_vertex([&gen]);
        let v01 = make_rand_vertex([&gen]);
        let v02 = make_rand_vertex([&gen]);
        let v10 = make_rand_vertex([&v00, &v01]);
        let v20 = make_rand_vertex([&v10]);
        let v30 = make_rand_vertex([&v20]);
        let mut dag = DAG::new(Config::default(), [&gen]).unwrap();
        dag.try_insert(&v00).unwrap();
        dag.try_insert(&v01).unwrap();
        dag.try_insert(&v02).unwrap();
        dag.try_insert(&v10).unwrap();
        dag.try_insert(&v20).unwrap();

        // Get ancestors of non-inserted vertex
        let mut actual = dag.get_active_ancestors(&v30).unwrap();
        let mut expected = vec![v00.hash(), v01.hash(), v10.hash(), v20.hash()];
        sort_range(&mut actual, 0..2);
        sort_range(&mut expected, 0..2);
        assert_eq!(actual, expected);

        // Insert the vertex and confirm the answer doesn't change
        dag.try_insert(&v30).unwrap();
        let mut actual = dag.get_active_ancestors(&v30).unwrap();
        let mut expected = vec![v00.hash(), v01.hash(), v10.hash(), v20.hash()];
        sort_range(&mut actual, 0..2);
        sort_range(&mut expected, 0..2);
        assert_eq!(actual, expected);

        // mark v10 as rejected
        let c_v10v10 = Constraint(v10.hash(), v10.hash());
        dag.state
            .get_mut(&c_v10v10.conflict_set_key())
            .unwrap()
            .decision
            .insert(c_v10v10, false);
        assert_matches!(
            dag.get_active_ancestors(&v30),
            Err(dag::Error::RejectedAncestor)
        );
        // mark v10 as accepted
        dag.state
            .get_mut(&c_v10v10.conflict_set_key())
            .unwrap()
            .decision
            .insert(c_v10v10, true);
        assert_eq!(dag.get_active_ancestors(&v30).unwrap(), [v20.hash()]);
    }

    #[test]
    fn get_active_ancestor_constraints() {
        fn sort_range<T: Ord + Copy>(v: &mut Vec<T>, range: std::ops::Range<usize>) {
            v.splice(range.clone(), {
                let mut tmp = v[range].to_vec();
                tmp.sort();
                tmp
            });
        }
        let gen = Arc::new(Vertex::empty());
        let v00 = make_rand_vertex([&gen]);
        let v01 = make_rand_vertex([&gen]);
        let v10 = make_rand_vertex([&v00, &v01]);
        let v20 = make_rand_vertex([&v10]);
        let mut dag = DAG::new(Config::default(), [&gen]).unwrap();
        dag.try_insert(&v00).unwrap();
        dag.try_insert(&v01).unwrap();

        let c_v00v00 = Constraint(v00.hash(), v00.hash());
        let c_v01v01 = Constraint(v01.hash(), v01.hash());
        let c_v00v01 = Constraint(v00.hash(), v01.hash());
        let c_v10v10 = Constraint(v10.hash(), v10.hash());

        // one of the parents (v10) has not been inserted yet
        assert_matches!(
            dag.get_active_ancestor_constraints(&v20),
            Err(dag::Error::NotFound)
        );

        dag.try_insert(&v10).unwrap();
        let mut expected = Vec::from([c_v00v00, c_v01v01, c_v00v01, c_v10v10]);
        let mut actual = dag.get_active_ancestor_constraints(&v20).unwrap();
        // There are a few permutations of expected ancestors, because constraints at same height
        // may appear in any order
        sort_range(&mut expected, 0..3);
        sort_range(&mut actual, 0..3);
        assert_eq!(actual, expected);

        // Should stop fetching ancestors at the first decided one
        dag.state
            .get_mut(&c_v10v10.conflict_set_key())
            .unwrap()
            .decision
            .insert(c_v10v10, true);
        assert_eq!(
            dag.get_active_ancestor_constraints(&v20)
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            []
        );
        // Should error if ancestor is rejected
        dag.state
            .get_mut(&c_v10v10.conflict_set_key())
            .unwrap()
            .decision
            .insert(c_v10v10, false);
        assert_matches!(
            dag.get_active_ancestor_constraints(&v20),
            Err(dag::Error::RejectedAncestor)
        );

        // Clear the decision for the next tests
        dag.state
            .get_mut(&c_v10v10.conflict_set_key())
            .unwrap()
            .decision
            .remove(&c_v10v10);

        // Test constraints for vertices of different heights. Should get all ancestors even if
        // some are already decided.
        let v02 = make_rand_vertex([&gen]);
        let v21 = make_rand_vertex([&v20, &v02]);
        dag.try_insert(&v02).unwrap();
        dag.try_insert(&v20).unwrap();
        dag.try_insert(&v21).unwrap();
        assert_eq!(
            dag.get_active_ancestor_constraints(&v02)
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            []
        );
    }

    #[test]
    fn check_vertex() {
        let gen = Arc::new(Vertex::empty());
        let v00 = make_rand_vertex([&gen]);
        let v01 = make_rand_vertex([&gen]);
        let v02 = make_rand_vertex([&gen]);
        let v10 = make_rand_vertex([&v01, &v02]);
        let x10 = make_rand_vertex([&v02, &v01]); // conflicts with v3
        let v20 = make_rand_vertex([&v00, &v10]);
        let b20 = make_rand_vertex([&v01, &v10]); // self referential parents
        let mut dag = DAG::new(Config::default(), [&gen]).unwrap();

        // Test for missing/waiting parents
        dag.insert(&v00);
        assert_matches!(dag.check_vertex(&v00), Err(dag::Error::AlreadyInserted));
        match dag.check_vertex(&v10) {
            Err(dag::Error::MissingParents(missing)) => {
                assert_eq!(missing, [v01.hash(), v02.hash()])
            }
            _ => panic!("Expected Error::MissingParents"),
        }
        dag.vertex.insert(v01.hash(), v01.clone()); // v1 is known but not processed
        match dag.check_vertex(&v10) {
            Err(dag::Error::MissingParents(missing)) => {
                assert_eq!(missing, [v02.hash()])
            }
            _ => panic!("Expected Error::MissingParents"),
        }
        dag.vertex.insert(v02.hash(), v02.clone()); // v2 is known but not processed
        match dag.check_vertex(&v10) {
            Err(dag::Error::WaitingOnParents(waiting)) => {
                assert_eq!(waiting, [v01.hash(), v02.hash()])
            }
            _ => panic!("Expected Error::WaitingOnParents"),
        }
        dag.insert(&v01);
        match dag.check_vertex(&v10) {
            Err(dag::Error::WaitingOnParents(waiting)) => {
                assert_eq!(waiting, [v02.hash()])
            }
            _ => panic!("Expected Error::WaitingOnParents"),
        }
        dag.insert(&v02);
        assert_matches!(dag.check_vertex(&v10), Ok(()));

        dag.insert(&v10);

        // Should be ok with parents of differnt heights
        assert_matches!(dag.check_vertex(&v20), Ok(()));

        // Test for self referential parents
        assert_matches!(
            dag.check_vertex(&b20),
            Err(dag::Error::SelfReferentialParent)
        );

        // Test incorrect height
        let mut x20 = Vertex::empty().with_parents([&v10]).unwrap();
        x20.height = 2; // Should have height 3
        match dag.check_vertex(&Arc::new(x20)) {
            Err(dag::Error::BadHeight(expected, actual)) => {
                assert_eq!(actual, 2);
                assert_eq!(expected, 3);
            }
            _ => panic!("Expected Error::BadHeight"),
        }

        // Catch conflicting parents
        dag.insert(&x10);
        let y20 = make_rand_vertex([&v10, &x10]); // illegal! references conflicting parents
        assert_matches!(
            dag.check_vertex(&y20),
            Err(dag::Error::ConflictingAncestors)
        );

        // Test OK with varying parent heights
        let v20 = make_rand_vertex([&v00, &v10]); // Ties v0 back into the graph
        assert_matches!(dag.check_vertex(&v20), Ok(()));
    }

    #[test]
    fn map_child() {
        let gen = Arc::new(Vertex::empty());
        let v0 = make_rand_vertex([&gen]);
        let v1 = make_rand_vertex([&gen]);
        let v2 = make_rand_vertex([&v0, &v1]);
        let mut dag = DAG::new(Config::default(), [&gen]).unwrap();
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
    fn retry_pending() {
        let gen = Arc::new(Vertex::empty());
        let v0 = make_rand_vertex([&gen]);
        let v1 = make_rand_vertex([&gen]);
        let v2 = make_rand_vertex([&v0, &v1]);
        let v3 = make_rand_vertex([&v2]);
        let mut dag = DAG::new(Config::default(), [&gen]).unwrap();

        // Skip v1, but attempt to insert everything else
        dag.try_insert(&v0).unwrap();
        match dag.try_insert(&v2) {
            Err(dag::Error::MissingParents(missing)) => {
                assert_eq!(missing, [v1.hash()])
            }
            e @ _ => panic!("unexpected result: {e:?}"),
        }
        match dag.try_insert(&v3) {
            Err(dag::Error::WaitingOnParents(missing)) => {
                assert_eq!(missing, [v2.hash()])
            }
            e @ _ => panic!("unexpected result: {e:?}"),
        }

        // Insert v1, and then confirm that all vertices successfully append on retry
        let waiting = dag.try_insert(&v1).unwrap();
        let successes = dag.retry_pending(&waiting).unwrap();
        assert!(successes.contains(&v2.hash()));
        assert!(successes.contains(&v3.hash()));
        assert_eq!(dag.get_frontier(), [v3]);
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
        let mut dag = DAG::new(Config::default(), [&gen]).unwrap();

        // Test for missing parents, waiting parents, and preference after insertions
        match dag.try_insert(&v2) {
            Err(dag::Error::MissingParents(missing)) => {
                assert_eq!(missing, [v0.hash(), v1.hash()])
            }
            e @ _ => panic!("unexpected result: {e:?}"),
        }
        assert_eq!(dag.try_insert(&v0).unwrap(), [v2.hash()].into());
        assert!(dag.get_frontier().iter().eq([&v0]));
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
        assert!(dag.get_frontier().iter().eq([&v0]));
        assert_eq!(dag.try_insert(&v1).unwrap(), [v2.hash(), x2.hash(),].into());
        assert!(dag.get_frontier().iter().eq([&v0, &v1]));
        assert!(dag.is_preferred(&v1.hash()).unwrap());
        match dag.try_insert(&v3) {
            Err(dag::Error::WaitingOnParents(waiting)) => {
                assert_eq!(waiting, [v2.hash()])
            }
            e @ _ => panic!("unexpected result: {e:?}"),
        }
        assert!(dag.get_frontier().iter().eq([&v0, &v1]));
        assert_eq!(dag.try_insert(&v2).unwrap(), [v3.hash()].into());
        assert!(dag.get_frontier().iter().eq([&v2]));
        assert!(dag.is_preferred(&v2.hash()).unwrap());
        assert_eq!(dag.try_insert(&v3).unwrap(), [].into());
        assert!(dag.get_frontier().iter().eq([&v3]));
        assert!(dag.is_preferred(&v3.hash()).unwrap());
        assert_eq!(dag.try_insert(&x2).unwrap(), [].into());
        assert!(dag.get_frontier().iter().eq([&v3]));
        // x2 should not be preferred, due to conflict
        assert!(dag.is_preferred(&x2.hash()).unwrap() == false);
        // b3 should fail due to illegal parent combination
        assert_matches!(dag.try_insert(&b3), Err(dag::Error::ConflictingAncestors));
    }

    #[test]
    fn record_query_result() {
        // TODO: need to check counters and confidences in addition to chits
        fn check_chits<'a, P>(dag: &DAG, pairs: P)
        where
            P: IntoIterator<Item = (&'a Arc<Vertex>, Option<bool>)>,
        {
            for (vx, expected) in pairs {
                let vhash = vx.hash();
                let unity = Constraint(vhash, vhash);
                let chit = dag
                    .state
                    .get(&unity.conflict_set_key())
                    .and_then(|s| s.chit.get(&unity));
                assert_eq!(chit, expected.as_ref());
            }
        }

        let gen = Arc::new(Vertex::empty());
        let v0 = make_rand_vertex([&gen]);
        let v1 = make_rand_vertex([&gen]);
        let v2 = make_rand_vertex([&v0, &v1]);
        let v3 = make_rand_vertex([&v2]);
        let mut dag = DAG::new(Config::default(), [&gen]).unwrap();
        dag.try_insert(&v0).unwrap();
        dag.try_insert(&v1).unwrap();

        // Basic success
        dag.record_query_result(&v0.hash(), true).unwrap();
        dag.record_query_result(&v1.hash(), false).unwrap();
        check_chits(
            &dag,
            [
                (&v0, Some(true)),
                (&v1, Some(false)),
                (&v2, None),
                (&v3, None),
            ],
        );

        // Error cases
        assert_matches!(
            dag.record_query_result(&v0.hash(), true),
            Err(dag::Error::AlreadyRecorded)
        );
        assert_matches!(
            dag.record_query_result(&v3.hash(), true),
            Err(dag::Error::NotFound)
        );
        check_chits(
            &dag,
            [
                (&v0, Some(true)),
                (&v1, Some(false)),
                (&v2, None),
                (&v3, None),
            ],
        );

        // will fail, but makes v3 known
        dag.try_insert(&v3).unwrap_err();
        // v2 still has not been inserted
        assert_matches!(
            dag.record_query_result(&v3.hash(), true),
            Err(dag::Error::WaitingOnInsert)
        );
        check_chits(
            &dag,
            [
                (&v0, Some(true)),
                (&v1, Some(false)),
                (&v2, None),
                (&v3, None),
            ],
        );

        dag.try_insert(&v2).unwrap();
        // v2 still has not been queried
        assert_matches!(
            dag.record_query_result(&v3.hash(), true),
            Err(dag::Error::WaitingOnInsert)
        );
        check_chits(
            &dag,
            [
                (&v0, Some(true)),
                (&v1, Some(false)),
                (&v2, Some(false)),
                (&v3, None),
            ],
        );

        // Insert v3. V2 has also been inserted, but still not queried
        dag.try_insert(&v3).unwrap();

        // Should succeed, even though queried out of order
        dag.record_query_result(&v3.hash(), true).unwrap();
        check_chits(
            &dag,
            [
                (&v0, Some(true)),
                (&v1, Some(false)),
                (&v2, Some(false)),
                (&v3, Some(true)),
            ],
        );

        // Should succeed even though recorded out of order
        dag.record_query_result(&v2.hash(), true).unwrap();
        check_chits(
            &dag,
            [
                (&v0, Some(true)),
                (&v1, Some(false)),
                (&v2, Some(true)),
                (&v3, Some(true)),
            ],
        );
    }

    #[test]
    fn is_preferred() {
        let gen = Arc::new(Vertex::empty());
        let v0 = make_rand_vertex([&gen]);
        let v1 = make_rand_vertex([&gen]);
        let v2 = make_rand_vertex([&v0, &v1]);
        let x2 = make_rand_vertex([&v1, &v0]); // Conflicts with v2
        let mut dag = DAG::new(Config::default(), [&gen]).unwrap();

        assert_matches!(dag.is_preferred(&v2.hash()), Err(dag::Error::NotFound));
        let _ = dag.try_insert(&v2); // V2 is known now, but still waiting on parents to process
        assert_matches!(
            dag.is_preferred(&v2.hash()),
            Err(dag::Error::WaitingOnInsert)
        );

        // Insert all parents of v2 and x2, so they will insert successfully now
        dag.insert(&v0);
        dag.insert(&v1);

        dag.insert(&v2);
        assert_matches!(dag.is_preferred(&v2.hash()), Ok(true));
        dag.insert(&x2);
        assert_matches!(dag.is_preferred(&x2.hash()), Ok(false));
    }

    #[test]
    fn query() {
        // TODO: need case: do not panic if vertex does not exist
        let gen = Arc::new(Vertex::empty());
        let v00 = make_rand_vertex([&gen]);
        let v01 = make_rand_vertex([&gen]);
        let v10 = make_rand_vertex([&v00, &v01]);
        let mut dag = DAG::new(Config::default(), [&gen]).unwrap();
        dag.try_insert(&v00).unwrap();
        dag.try_insert(&v01).unwrap();
        assert_matches!(dag.query(&v10.hash()), Err(dag::Error::NotFound));

        dag.try_insert(&v10).unwrap();

        // Test initial state of the dag after all inserted
        assert_matches!(dag.query(&gen.hash()), Ok((true, true)));
        assert_matches!(dag.query(&v00.hash()), Ok((true, false)));
        assert_matches!(dag.query(&v01.hash()), Ok((true, false)));
        assert_matches!(dag.query(&v10.hash()), Ok((true, false)));

        let c_v00v00 = Constraint(v00.hash(), v00.hash());
        let c_v01v01 = Constraint(v01.hash(), v01.hash());
        let c_v00v01 = Constraint(v00.hash(), v01.hash());
        let c_v10v10 = Constraint(v10.hash(), v10.hash());

        // helpers to set the state of a constraint
        let set_decision = |dag: &mut DAG, c: Constraint, d: Option<bool>| {
            let c_state = dag.state.get_mut(&c.conflict_set_key()).unwrap();
            if let Some(decision) = d {
                c_state.decision.insert(c, decision);
            } else {
                c_state.decision.remove(&c);
            }
        };
        let set_preferred = |dag: &mut DAG, c: Constraint, p: bool| {
            dag.state.get_mut(&c.conflict_set_key()).unwrap().preferred =
                if p { c } else { c.conflict().unwrap() };
        };

        // Test with all "accepted"
        set_decision(&mut dag, c_v00v00, Some(true));
        set_decision(&mut dag, c_v01v01, Some(true));
        set_decision(&mut dag, c_v00v01, Some(true));
        set_decision(&mut dag, c_v10v10, Some(true));
        assert_matches!(dag.query(&v10.hash()), Ok((true, true)));

        // Should still be "accepted" even if all others are rejected or undecided
        set_decision(&mut dag, c_v00v00, Some(false));
        set_decision(&mut dag, c_v01v01, Some(false));
        set_decision(&mut dag, c_v00v01, Some(false));
        set_decision(&mut dag, c_v10v10, Some(true));
        assert_matches!(dag.query(&v10.hash()), Ok((true, true)));
        set_decision(&mut dag, c_v00v00, None);
        set_decision(&mut dag, c_v01v01, Some(false));
        set_decision(&mut dag, c_v00v01, None);
        set_preferred(&mut dag, c_v00v01, false);
        set_decision(&mut dag, c_v10v10, Some(true));
        assert_matches!(dag.query(&v10.hash()), Ok((true, true)));

        // Should be rejected even if all others are accepted or undecided
        set_decision(&mut dag, c_v00v00, Some(true));
        set_decision(&mut dag, c_v01v01, Some(true));
        set_decision(&mut dag, c_v00v01, Some(true));
        set_decision(&mut dag, c_v10v10, Some(false));
        assert_matches!(dag.query(&v10.hash()), Ok((false, true)));
        set_decision(&mut dag, c_v00v00, Some(true));
        set_decision(&mut dag, c_v01v01, None);
        set_decision(&mut dag, c_v00v01, Some(true));
        set_decision(&mut dag, c_v10v10, Some(false));
        assert_matches!(dag.query(&v10.hash()), Ok((false, true)));

        // Should be undecided, but strongly preferred, if all ancesters are preferred
        set_decision(&mut dag, c_v00v00, Some(true));
        set_decision(&mut dag, c_v01v01, Some(true));
        set_decision(&mut dag, c_v00v01, Some(true));
        set_preferred(&mut dag, c_v00v01, true);
        set_decision(&mut dag, c_v10v10, None);
        assert_matches!(dag.query(&v10.hash()), Ok((true, false)));
        set_decision(&mut dag, c_v00v00, None);
        set_decision(&mut dag, c_v01v01, None);
        set_decision(&mut dag, c_v00v01, None);
        set_preferred(&mut dag, c_v00v01, true);
        set_decision(&mut dag, c_v10v10, None);
        assert_matches!(dag.query(&v10.hash()), Ok((true, false)));
        set_decision(&mut dag, c_v00v00, None);
        set_decision(&mut dag, c_v01v01, None);
        set_decision(&mut dag, c_v00v01, None);
        set_preferred(&mut dag, c_v00v01, false);
        set_decision(&mut dag, c_v10v10, None);
        assert_matches!(dag.query(&v10.hash()), Ok((false, false)));
    }

    #[test]
    fn get_known_children() {
        let gen = Arc::new(Vertex::empty());
        let v10 = make_rand_vertex([&gen]);
        let v11 = make_rand_vertex([&gen]);
        let mut dag = DAG::new(Config::default(), [&gen]).unwrap();
        assert_eq!(dag.get_known_children(&gen.hash()).unwrap(), HashSet::new());
        assert_matches!(
            dag.get_known_children(&v10.hash()),
            Err(dag::Error::NotFound)
        );
        dag.insert(&v10);
        assert!(dag
            .get_known_children(&gen.hash())
            .unwrap()
            .into_iter()
            .eq([v10.hash()]));
        dag.insert(&v11);
        assert!(dag
            .get_known_children(&gen.hash())
            .unwrap()
            .into_iter()
            .sorted()
            .eq([v10.hash(), v11.hash()].into_iter().sorted()));
    }

    #[test]
    fn new_conflict_set() {
        let gen = Arc::new(Vertex::empty());
        let v00 = make_rand_vertex([&gen]);
        let v01 = make_rand_vertex([&gen]);
        let v20 = make_rand_vertex([&v00, &v01]);

        // New conflict set for binary constraint
        let constraint = Constraint(v20.hash(), v01.hash()); // Binary constraint between v2 & v1
        let c_parents = v20.parent_constraints().collect::<HashSet<_>>();
        let cs = ConflictSet::new(constraint, c_parents.clone());
        assert_eq!(cs.preferred, constraint);
        assert_eq!(cs.last, constraint);
        assert_eq!(cs.chit.len(), 2);
        assert_eq!(cs.chit[&constraint], false);
        assert_eq!(cs.chit[&constraint.conflict().unwrap()], false);
        assert_eq!(cs.confidence.len(), 2);
        assert_eq!(cs.confidence[&constraint], 0);
        assert_eq!(cs.confidence[&constraint.conflict().unwrap()], 0);
        assert_eq!(cs.count.len(), 2);
        assert_eq!(cs.count[&constraint], 0);
        assert_eq!(cs.count[&constraint.conflict().unwrap()], 0);
        assert_eq!(cs.parents.len(), 2);
        assert!(cs.parents[&constraint]
            .iter()
            .sorted()
            .eq(v20.parent_constraints().sorted().as_ref()));
        assert_eq!(cs.parents[&constraint.conflict().unwrap()].len(), 0);
        assert_eq!(cs.children.len(), 2);
        assert_eq!(cs.children[&constraint].len(), 0);
        assert_eq!(cs.children[&constraint.conflict().unwrap()].len(), 0);
        assert_eq!(cs.decision.len(), 0); // Decision is intentionally empty

        // New conflict set for unity constraint
        let constraint = Constraint(v20.hash(), v20.hash()); // Unity contraint on v2
        let c_parents = v20.parent_constraints().collect::<HashSet<_>>();
        let cs = ConflictSet::new(constraint, c_parents.clone());
        assert_eq!(cs.preferred, constraint);
        assert_eq!(cs.last, constraint);
        assert_eq!(cs.chit.len(), 1);
        assert_eq!(cs.chit[&constraint], false);
        assert_eq!(cs.confidence.len(), 1);
        assert_eq!(cs.confidence[&constraint], 0);
        assert_eq!(cs.count.len(), 1);
        assert_eq!(cs.count[&constraint], 0);
        assert_eq!(cs.parents.len(), 1);
        assert!(cs.parents[&constraint]
            .iter()
            .sorted()
            .eq(v20.parent_constraints().sorted().as_ref()));
        assert_eq!(cs.children.len(), 1);
        assert_eq!(cs.children[&constraint].len(), 0);
        assert_eq!(cs.decision.len(), 0); // Decision is intentionally empty
    }
}
