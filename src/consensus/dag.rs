use super::conflict_set::ConflictGraph;
use crate::{params, vertex, Vertex, VertexHash, WireFormat};
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

/// Implementation of SESAME DAG
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DAG {
    /// Configuration parameters
    config: Config,

    /// Map of every [`Vertex`] in the [`DAG`]
    vertices: HashMap<VertexHash, Arc<Vertex>>,
    // TODO: these HashMaps should be HashSets with hasher = WireFormat::hash
    /// Map of state variables for each [`Vertex`] in the [`DAG`]` (chit and confidence)
    chitconf: HashMap<VertexHash, (usize, usize)>,

    /// Map of preferences for every [`Vertex`] in the [`DAG`]
    preferences: HashMap<VertexHash, bool>,

    /// Map of known children for each vertex in the [`DAG`]
    children: HashMap<VertexHash, HashMap<VertexHash, Arc<Vertex>>>,

    /// Graph of [`Vertex`] conflicts
    conflicts: ConflictGraph,

    /// Vertices which define the frontier of the [`DAG`]
    /// The frontier is ordered according to latest ordering preference
    frontier: HashSet<VertexHash>,
}

impl DAG {
    /// Create a new [`DAG`] with the given [`Config`]
    pub fn new(config: Config) -> DAG {
        DAG {
            config,
            vertices: HashMap::new(),
            chitconf: HashMap::new(),
            preferences: HashMap::new(),
            children: HashMap::new(),
            conflicts: ConflictGraph::new(),
            frontier: HashSet::new(),
        }
    }

    /// Before inserting a vertex into the [`DAG`] it must pass these checks
    fn check_vertex(&self, vx: &Vertex) -> Result<()> {
        // Rule out any obviously insane vertices
        vx.sanity_checks()?;

        // Check the child map to make sure this vertex doesn't already exist
        if self.vertices.contains_key(&vx.hash()) {
            return Err(Error::AlreadyExists);
        }

        // Make sure parents exist
        let missing: Vec<_> = vx
            .parents
            .iter()
            .copied()
            .filter(|h| !self.vertices.contains_key(h))
            .collect();
        if !missing.is_empty() {
            return Err(Error::MissingParents(missing));
        }

        // Make sure parents have been successfully inserted and not just waiting in the child map.
        // Chits and confidence are only rewarded once a vertex has been inserted, so we check for
        // existence of each parent's chitconf as an indication that each has been inserted.
        let waiting: Vec<_> = vx
            .parents
            .iter()
            .copied()
            .filter(|h| {
                !self.chitconf.contains_key(h)
                    || !self.children.contains_key(h)
                    || !self.preferences.contains_key(h)
            })
            .collect();
        if !waiting.is_empty() {
            return Err(Error::WaitingOnParents(waiting));
        }

        let parent_height = self.vertices[vx.parents.first().unwrap()].height;
        if !vx
            .parents
            .iter()
            .all(|p| self.vertices[p].height == parent_height)
        {
            return Err(Error::BadParentHeight);
        }

        let expected_height = parent_height + 1;
        if expected_height != vx.height {
            return Err(Error::BadHeight(vx.height, expected_height));
        }
        Ok(())
    }

    /// Add this vertex as a known child to each of its parents
    fn map_child(&mut self, vx: &Arc<Vertex>) {
        for &p in &vx.parents {
            if let Some(children) = self.children.get_mut(&p) {
                children.insert(vx.hash(), vx.clone());
            } else {
                self.children
                    .insert(p, once((vx.hash(), vx.clone())).collect());
            }
        }
    }

    /// Return true if the specified vertex is preferred over all its conflicts
    pub fn is_preferred(&self, vx: &Vertex) -> Result<bool> {
        self.preferences
            .get(&vx.hash())
            .copied()
            .ok_or(Error::NotFound)
    }

    /// Recursively recompute the state of the given [`Vertex`], and each of its undecided ancestors
    fn recompute_at(&mut self, vhash: &VertexHash) -> Result<()> {
        if !self.vertices.contains_key(vhash) {
            return Err(Error::NotFound);
        }
        if let Some(changes) = self.recompute_confidences(vhash) {
            // Recompute preferences, and collect the changes
            let mut pref = HashSet::with_capacity(changes.len());
            let mut nonpref = HashSet::with_capacity(changes.len());
            for changed in changes {
                let (updated, preferred) = self.recompute_preference(&changed);
                if updated && preferred {
                    pref.insert(changed);
                } else if updated && !preferred {
                    nonpref.insert(changed);
                }
            }

            // Remove any non-preferred vertices from the frontier
            self.frontier.extract_if(|vhash| !self.preferences[vhash]);

            // Gather any vertices which may now be at the frontier, and add them if so
            nonpref
                .into_iter()
                .map(|vhash| self.vertices[&vhash].parents.iter())
                .flatten()
                .chain(pref.iter())
                .for_each(|candidate| {
                    // If no children are preferred, add it to the frontier
                    if !self.children[candidate]
                        .keys()
                        .any(|child| self.preferences[child])
                    {
                        self.frontier.insert(*candidate);
                    }
                })
        }
        Ok(())
    }

    /// Recursively recompute the confidences of the given [`Vertex`], and each of its undecided
    /// ancestors, returning the hashes of every vertex which changed, ordered from oldest to
    /// youngest
    fn recompute_confidences(&mut self, vhash: &VertexHash) -> Option<Vec<VertexHash>> {
        // Compute the new confidence value
        let progeny_confidence: usize = self.children[vhash]
            .keys()
            .map(|child| self.chitconf[child].1)
            .sum();

        // Update the confidence
        let chitconf = self.chitconf.get_mut(vhash).unwrap();
        let new_confidence = usize::min(
            chitconf.0 + progeny_confidence, // confidence(v) = v.chit + confidence(v.progeny)
            self.config.acceptance_threshold,
        );
        let changed = chitconf.1 != new_confidence;
        chitconf.1 = new_confidence;

        // If the state changed, recurse into children
        // TODO: do this in parallel
        if changed {
            Some(
                self.vertices[vhash]
                    .clone()
                    .parents
                    .iter()
                    .filter_map(|parent| self.recompute_confidences(&parent))
                    .flatten()
                    .collect(),
            )
        } else {
            None
        }
    }

    /// Recompute the preference of the given [`Vertex`], and return a tuple indicating if the
    /// preference has changed and the latest preference
    fn recompute_preference(&mut self, vhash: &VertexHash) -> (bool, bool) {
        let vx = &self.vertices[vhash];
        let conflicts = self.conflicts.conflicts_of(vx);

        // Compute the max confidence of conflicts
        let max_conflict_confidence = conflicts
            .keys()
            .map(|vhash| self.chitconf[vhash].1)
            .max()
            .unwrap_or(0);

        // Determine if this vertex is preferred over its conflicts
        let confidence = self.chitconf[vhash].1;
        let old_preference = self.preferences[vhash];
        let new_preference = if !vx.parents.iter().all(|parent| self.preferences[parent]) {
            // If any parents are not preferred, this vertex cannot be preferred
            false
        } else if confidence == max_conflict_confidence {
            // If confidence is tied with conflicts, keep the original preference
            old_preference
        } else {
            // Only preferred if parents are preferred and confidence exceeds conflicts
            confidence > max_conflict_confidence
        };

        // If the preference has changed, update it
        let updated = old_preference != new_preference;
        if updated {
            self.preferences.insert(*vhash, new_preference);

            // If this vertex just became preferred, reset all conflicts
            if new_preference {
                for &&vhash in conflicts.keys() {
                    self.preferences.insert(vhash, false);
                    self.chitconf.insert(vhash, (0, 0));
                }
            }
        }
        (updated, new_preference)
    }

    /// Insert a [`Vertex`] without checking. This should only be used to init the [`DAG`] on
    /// restart
    pub fn force_insert(&mut self, vx: &Arc<Vertex>) -> (bool, Option<Vec<Arc<Vertex>>>) {
        // Initialize state variables for this vertex
        self.map_child(vx);
        let _ = self.children.try_insert(vx.hash(), HashMap::new());
        self.conflicts.insert(vx);
        self.vertices.try_insert(vx.hash(), vx.clone()).unwrap();
        self.chitconf.try_insert(vx.hash(), (0, 0)).unwrap();
        self.preferences.try_insert(vx.hash(), false).unwrap();

        // Recompute states
        self.recompute_at(&vx.hash()).unwrap();

        (
            self.is_preferred(vx).unwrap(),
            self.children
                .get(&vx.hash())
                .and_then(|c| Some(c.values().cloned().collect())),
        )
    }

    /// Insert a vertex into the [`DAG`]. Returns boolean indicating
    /// if the vertex is preferred or not, as well as a list of known children waiting to be
    /// inserted.
    pub fn try_insert(&mut self, vx: &Arc<Vertex>) -> Result<(bool, Option<Vec<Arc<Vertex>>>)> {
        // TODO: this should be thread safe...need mutex on DAG?
        // Check if the vertex may be inserted
        match self.check_vertex(vx) {
            res @ Err(Error::MissingParents(_)) => {
                // Add vertex as known child, even if we are missing some of its parents
                self.map_child(vx);
                res
            }
            res @ _ => res,
        }?;

        Ok(self.force_insert(vx))
    }

    /// Award a chit to the specified vertex, according to the Avalanche protocol
    pub fn award_chit(&mut self, vhash: &VertexHash) -> Result<()> {
        self.chitconf.get_mut(vhash).ok_or(Error::NotFound)?.0 = 1;
        self.recompute_at(vhash)?;
        Ok(())
    }

    /// Get the latest vertices which have no children, in the order we've observed them
    pub fn get_frontier(&mut self) -> Vec<VertexHash> {
        self.frontier
            .iter()
            .map(|vhash| (vhash, self.vertices[vhash].timestamp))
            .sorted_by_key(|(_vhash, time)| *time)
            .map(|(vhash, _time)| vhash)
            .copied()
            .collect()
    }
}

#[cfg(test)]
mod test {
    use super::{Config, DAG};
    use crate::{
        consensus::{conflict_set::ConflictGraph, dag},
        params,
        vertex::test_vertex,
        Vertex, WireFormat,
    };
    use std::{
        assert_matches::assert_matches,
        collections::{HashMap, HashSet},
        sync::Arc,
    };

    #[test]
    fn new_config() {
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
        assert_eq!(dag.vertices, HashMap::new());
        assert_eq!(dag.chitconf, HashMap::new());
        assert_eq!(dag.preferences, HashMap::new());
        assert_eq!(dag.children, HashMap::new());
        assert_eq!(dag.conflicts, ConflictGraph::new());
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

        dag.vertices.insert(gen.hash(), gen.clone());
        dag.chitconf.insert(gen.hash(), (0, 0));
        dag.preferences.insert(gen.hash(), false);
        dag.children.insert(gen.hash(), HashMap::new()); // Genesis is "processed"
        dag.vertices.insert(c0.hash(), c0.clone());
        dag.chitconf.insert(c0.hash(), (0, 0));
        dag.preferences.insert(c0.hash(), false);
        dag.children.insert(c0.hash(), HashMap::new()); // c0 is "processed"
        assert_matches!(dag.check_vertex(&c0), Err(dag::Error::AlreadyExists));
        match dag.check_vertex(&c3) {
            Err(dag::Error::MissingParents(missing)) => {
                assert_eq!(missing, [c1.hash(), c2.hash()])
            }
            _ => panic!("Expected Error::MissingParents"),
        }
        dag.vertices.insert(c1.hash(), c1.clone()); // c1 is known but not processed
        match dag.check_vertex(&c3) {
            Err(dag::Error::MissingParents(missing)) => {
                assert_eq!(missing, [c2.hash()])
            }
            _ => panic!("Expected Error::MissingParents"),
        }
        dag.vertices.insert(c2.hash(), c2.clone()); // c2 is known but not processed
        match dag.check_vertex(&c3) {
            Err(dag::Error::WaitingOnParents(waiting)) => {
                assert_eq!(waiting, [c1.hash(), c2.hash()])
            }
            _ => panic!("Expected Error::WaitingOnParents"),
        }
        dag.vertices.insert(c1.hash(), c1.clone());
        dag.chitconf.insert(c1.hash(), (0, 0));
        dag.preferences.insert(c1.hash(), false);
        dag.children.insert(c1.hash(), HashMap::new()); // c1 is "processed"
        match dag.check_vertex(&c3) {
            Err(dag::Error::WaitingOnParents(waiting)) => {
                assert_eq!(waiting, [c2.hash()])
            }
            _ => panic!("Expected Error::WaitingOnParents"),
        }
        dag.vertices.insert(c2.hash(), c2.clone());
        dag.chitconf.insert(c2.hash(), (0, 0));
        dag.preferences.insert(c2.hash(), false);
        dag.children.insert(c2.hash(), HashMap::new()); // c2 is "processed"
        assert_matches!(dag.check_vertex(&c3), Ok(()));

        let c4 = test_vertex([&gen, &c1]); // Parents have mismatched height
        assert_matches!(dag.check_vertex(&c4), Err(dag::Error::BadParentHeight));

        dag.vertices.insert(c3.hash(), c3.clone());
        dag.chitconf.insert(c3.hash(), (0, 0));
        dag.preferences.insert(c3.hash(), false);
        dag.children.insert(c3.hash(), HashMap::new()); // c3 is "processed"
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
        todo!();
    }

    #[test]
    fn is_preferred() {
        todo!();
    }

    #[test]
    fn recompute_at() {
        todo!();
    }

    #[test]
    fn recompute_confidences() {
        todo!();
    }

    #[test]
    fn recompute_preferences() {
        todo!();
    }

    #[test]
    fn try_insert() {
        todo!()
    }

    #[test]
    fn award_chit() {
        todo!()
    }

    #[test]
    fn get_frontier() {
        todo!()
    }
}
