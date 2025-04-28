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
    // TODO: chit should be bool
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

    /// Recursively collect the full progeny of the specified vertex
    fn collect_progeny(&self, vhash: &VertexHash) -> HashSet<VertexHash> {
        self.children[vhash]
            .keys()
            .map(|child| {
                let mut tmp = self.collect_progeny(child);
                tmp.insert(*child);
                tmp.into_iter()
            })
            .flatten()
            .collect()
    }

    /// Recursively recompute the state of the given [`Vertex`], and each of its undecided ancestors
    fn recompute_at(&mut self, vhash: &VertexHash) {
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
    }

    /// Recursively recompute the confidences of the given [`Vertex`], and each of its undecided
    /// ancestors, returning the hashes of every vertex which changed, ordered from oldest to
    /// youngest
    fn recompute_confidences(&mut self, vhash: &VertexHash) -> Option<Vec<VertexHash>> {
        // TODO: this method in dire need of performance optimization
        // Count the number of chits in the progeny
        let progeny_chits: usize = self
            .collect_progeny(vhash)
            .iter()
            .map(|vhash| self.chitconf[vhash].0)
            .sum();

        // Update the confidence
        let chitconf = self.chitconf.get_mut(vhash).unwrap();
        let new_confidence =
            usize::min(chitconf.0 + progeny_chits, self.config.acceptance_threshold);
        let changed = chitconf.1 != new_confidence;
        chitconf.1 = new_confidence;

        // If the state changed, recurse into children
        // TODO: do this in parallel
        if changed {
            Some(
                once(*vhash)
                    .chain(
                        self.vertices[vhash]
                            .clone()
                            .parents
                            .iter()
                            .filter_map(|parent| self.recompute_confidences(&parent))
                            .flatten()
                            .unique(),
                    )
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
        self.recompute_at(&vx.hash());

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
        self.recompute_at(vhash);
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
    use itertools::Itertools;

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
        let gen = Arc::new(Vertex::empty());
        let c0 = test_vertex([&gen]);
        let c1 = test_vertex([&gen]);
        let c2 = test_vertex([&c0, &c1]);
        let mut dag = DAG::new(Config::default());
        dag.children.insert(gen.hash(), HashMap::new());
        dag.children.insert(c0.hash(), HashMap::new());
        dag.children.insert(c1.hash(), HashMap::new());
        dag.children.insert(c2.hash(), HashMap::new());
        assert_eq!(dag.children[&gen.hash()].len(), 0);
        assert_eq!(dag.children[&c0.hash()].len(), 0);
        assert_eq!(dag.children[&c1.hash()].len(), 0);
        assert_eq!(dag.children[&c2.hash()].len(), 0);
        dag.map_child(&c0);
        assert_eq!(dag.children[&gen.hash()].len(), 1);
        assert_eq!(dag.children[&c0.hash()].len(), 0);
        assert_eq!(dag.children[&c1.hash()].len(), 0);
        assert_eq!(dag.children[&c2.hash()].len(), 0);
        dag.map_child(&c1);
        assert_eq!(dag.children[&gen.hash()].len(), 2);
        assert_eq!(dag.children[&c0.hash()].len(), 0);
        assert_eq!(dag.children[&c1.hash()].len(), 0);
        assert_eq!(dag.children[&c2.hash()].len(), 0);
        dag.map_child(&c2);
        assert_eq!(dag.children[&gen.hash()].len(), 2);
        assert_eq!(dag.children[&c0.hash()].len(), 1);
        assert_eq!(dag.children[&c1.hash()].len(), 1);
        assert_eq!(dag.children[&c2.hash()].len(), 0);
    }

    #[test]
    fn is_preferred() {
        let gen = Arc::new(Vertex::empty());
        let c0 = test_vertex([&gen]);
        let c1 = test_vertex([&gen]);
        let mut dag = DAG::new(Config::default());
        dag.preferences.insert(gen.hash(), true);
        dag.preferences.insert(c0.hash(), false);
        assert_eq!(dag.is_preferred(&gen).unwrap(), true);
        assert_eq!(dag.is_preferred(&c0).unwrap(), false);
        assert_matches!(dag.is_preferred(&c1), Err(dag::Error::NotFound));
    }

    #[test]
    fn collect_progeny() {
        todo!();
    }

    #[test]
    fn recompute_at() {
        // Set up test vertices and dag
        let gen = Arc::new(Vertex::empty());
        let a00 = test_vertex([&gen]);
        let a01 = test_vertex([&gen]);
        let a02 = test_vertex([&gen]);
        // Create conflicting sets a & b
        let b10 = test_vertex([&a00, &a01]);
        let b11 = test_vertex([&a00, &a02]);
        let b20 = test_vertex([&b10]);
        let b21 = test_vertex([&b10, &b11]);
        let b30 = test_vertex([&b21, &b20]);
        let c10 = test_vertex([&a01, &a00]);
        let c11 = test_vertex([&a02, &a00]);
        let c20 = test_vertex([&c10]);
        let c21 = test_vertex([&c10, &c11]);
        let c30 = test_vertex([&c21, &c20]);

        // Helper to advance the DAG with new vertices
        let add_to_dag = |dag: &mut DAG, vx: &Arc<Vertex>| {
            dag.vertices.insert(vx.hash(), vx.clone());
            dag.children.insert(vx.hash(), HashMap::new());
            dag.map_child(&vx);
            dag.chitconf.insert(vx.hash(), (0, 0));
            dag.preferences.insert(vx.hash(), false);
            dag.conflicts.insert(&vx);
        };

        // Helper to assert that all confidences match expected
        let assert_confs_and_prefs = |dag: &DAG, expected: &[(&Arc<Vertex>, (usize, bool))]| {
            for (vhash, conf, pref) in expected
                .iter()
                .map(|(vx, confpref)| (vx.hash(), confpref.0, confpref.1))
            {
                assert_eq!(dag.chitconf[&vhash].1, conf);
                assert_eq!(dag.preferences[&vhash], pref);
            }
        };

        // Helper to set the confidence of a vertex
        let set_chit = |dag: &mut DAG, vx: &Arc<Vertex>, chit: usize| {
            dag.chitconf.get_mut(&vx.hash()).unwrap().0 = chit;
        };

        // Insert everything into the DAG
        const MAX_CONF: usize = 9;
        let mut dag = DAG::new(Config {
            acceptance_threshold: MAX_CONF,
        });
        add_to_dag(&mut dag, &gen);
        add_to_dag(&mut dag, &a00);
        add_to_dag(&mut dag, &a01);
        add_to_dag(&mut dag, &a02);
        add_to_dag(&mut dag, &b10);
        add_to_dag(&mut dag, &b11);
        add_to_dag(&mut dag, &b20);
        add_to_dag(&mut dag, &b21);
        add_to_dag(&mut dag, &b30);
        add_to_dag(&mut dag, &c10);
        add_to_dag(&mut dag, &c11);
        add_to_dag(&mut dag, &c20);
        add_to_dag(&mut dag, &c21);
        add_to_dag(&mut dag, &c30);

        // Basic test
        set_chit(&mut dag, &gen, 1); // High enough to always be preferred
        dag.recompute_at(&gen.hash());
        assert_eq!(dag.preferences[&gen.hash()], true);
        assert_eq!(dag.chitconf[&gen.hash()], (1, 1));
        assert_confs_and_prefs(
            &dag,
            &[
                (&gen, (1, true)),
                (&a00, (0, false)),
                (&a01, (0, false)),
                (&a02, (0, false)),
                (&b10, (0, false)),
                (&b11, (0, false)),
                (&b20, (0, false)),
                (&b21, (0, false)),
                (&b30, (0, false)),
                (&c10, (0, false)),
                (&c11, (0, false)),
                (&c20, (0, false)),
                (&c21, (0, false)),
                (&c30, (0, false)),
            ],
        );

        // Award chits to "b" sub tree
        set_chit(&mut dag, &a00, 1);
        set_chit(&mut dag, &a01, 1);
        set_chit(&mut dag, &a02, 1);
        set_chit(&mut dag, &b10, 1);
        set_chit(&mut dag, &b11, 1);
        set_chit(&mut dag, &b20, 1);
        set_chit(&mut dag, &b21, 1);
        set_chit(&mut dag, &b30, 1);
        dag.recompute_at(&b30.hash());
        assert_confs_and_prefs(
            &dag,
            &[
                (&gen, (9, true)),
                (&a00, (6, true)),
                (&a01, (5, true)),
                (&a02, (4, true)),
                (&b10, (4, true)),
                (&b11, (3, true)),
                (&b20, (2, true)),
                (&b21, (2, true)),
                (&b30, (1, true)),
                (&c10, (0, false)),
                (&c11, (0, false)),
                (&c20, (0, false)),
                (&c21, (0, false)),
                (&c30, (0, false)),
            ],
        );

        // Awarding chits to "c" sub tree should not change the results
        set_chit(&mut dag, &c10, 1);
        set_chit(&mut dag, &c11, 1);
        set_chit(&mut dag, &c20, 1);
        set_chit(&mut dag, &c21, 1);
        set_chit(&mut dag, &c30, 1);
        dag.recompute_at(&c30.hash());
        assert_confs_and_prefs(
            &dag,
            &[
                (&gen, (9, true)),
                (&a00, (6, true)),
                (&a01, (5, true)),
                (&a02, (4, true)),
                (&b10, (4, true)),
                (&b11, (3, true)),
                (&b20, (2, true)),
                (&b21, (2, true)),
                (&b30, (1, true)),
                (&c10, (0, false)),
                (&c11, (0, false)),
                (&c20, (0, false)),
                (&c21, (0, false)),
                (&c30, (0, false)),
            ],
        );

        // Extend progeny of c30 to flip "c" sub tree into preference
        let c40 = test_vertex([&c21, &c20]);
        add_to_dag(&mut dag, &c40);
        set_chit(&mut dag, &c40, 1);
        dag.recompute_at(&c40.hash());
        assert_confs_and_prefs(
            &dag,
            &[
                (&gen, (MAX_CONF, true)),
                (&a00, (7, true)),
                (&a01, (6, true)),
                (&a02, (5, true)),
                (&b10, (0, true)),
                (&b11, (0, true)),
                (&b20, (0, true)),
                (&b21, (0, true)),
                (&b30, (0, true)),
                (&c10, (5, false)),
                (&c11, (4, false)),
                (&c20, (3, false)),
                (&c21, (3, false)),
                (&c30, (2, false)),
                (&c40, (1, false)),
            ],
        );
    }

    #[test]
    fn recompute_confidences() {
        // Set up test vertices and dag
        let gen = Arc::new(Vertex::empty());
        let c0 = test_vertex([&gen]);
        let c1 = test_vertex([&gen]);
        let c2 = test_vertex([&c0]);
        let c3 = test_vertex([&c0, &c1]);
        const MAX_CONF: usize = 6;
        let mut dag = DAG::new(Config {
            acceptance_threshold: MAX_CONF,
        });

        // Helper to advance the DAG with new vertices
        let add_to_dag = |dag: &mut DAG, vx: &Arc<Vertex>| {
            dag.vertices.insert(vx.hash(), vx.clone());
            dag.children.insert(vx.hash(), HashMap::new());
            dag.map_child(&vx);
            dag.chitconf.insert(vx.hash(), (0, 0));
        };

        // Helper to assert that the expected vertices are updated in a recomputation
        let test_recompute =
            |dag: &mut DAG, start: &Arc<Vertex>, expected_updates: &[&Arc<Vertex>]| {
                if let Some(modified) = dag.recompute_confidences(&start.hash()) {
                    println!("---- recompute ----");
                    assert!(modified
                        .into_iter()
                        .inspect(|vhash| println!(":::: {vhash}"))
                        .sorted()
                        .eq(expected_updates.into_iter().map(|vx| vx.hash()).sorted()));
                } else {
                    if !expected_updates.is_empty() {
                        panic!("expected modifications")
                    }
                }
            };

        // Helper to assert that all confidences match expected
        let assert_confidences = |dag: &DAG, expected: &[(&Arc<Vertex>, usize)]| {
            for (vhash, conf) in expected.iter().map(|(vx, conf)| (vx.hash(), conf)) {
                assert_eq!(dag.chitconf[&vhash].1, *conf);
            }
        };

        // Add everything to the dag with 0 chit & confidence
        add_to_dag(&mut dag, &gen);
        add_to_dag(&mut dag, &c0);
        add_to_dag(&mut dag, &c1);
        add_to_dag(&mut dag, &c2);
        add_to_dag(&mut dag, &c3);

        // Everything should have zero confidence, and no changes
        test_recompute(&mut dag, &c3, &[]);
        assert_confidences(&dag, &[(&gen, 0), (&c0, 0), (&c1, 0), (&c2, 0), (&c3, 0)]);

        // Assign a chit to c3 and recompute
        dag.chitconf.insert(c3.hash(), (1, 0)); // Assign chit to c3
        test_recompute(&mut dag, &c3, &[&c3, &c1, &c0, &gen]);
        assert_confidences(&dag, &[(&gen, 1), (&c0, 1), (&c1, 1), (&c2, 0), (&c3, 1)]);

        // Recompute again should result in no updates
        test_recompute(&mut dag, &c3, &[]);
        assert_confidences(&dag, &[(&gen, 1), (&c0, 1), (&c1, 1), (&c2, 0), (&c3, 1)]);

        // Assign a chit to c2 and recompute
        dag.chitconf.insert(c2.hash(), (1, 0)); // Assign chit to c2
        test_recompute(&mut dag, &c2, &[&c2, &c0, &gen]);
        assert_confidences(&dag, &[(&gen, 2), (&c0, 2), (&c1, 1), (&c2, 1), (&c3, 1)]);

        // Assign a chit to c1 and recompute
        dag.chitconf.insert(c1.hash(), (1, 0)); // Assign chit to c1
        test_recompute(&mut dag, &c1, &[&c1, &gen]);
        assert_confidences(&dag, &[(&gen, 3), (&c0, 2), (&c1, 2), (&c2, 1), (&c3, 1)]);

        // Assign a chit to c0 and recompute
        dag.chitconf.insert(c0.hash(), (1, 0)); // Assign chit to c0
        test_recompute(&mut dag, &c0, &[&c0, &gen]);
        assert_confidences(&dag, &[(&gen, 4), (&c0, 3), (&c1, 2), (&c2, 1), (&c3, 1)]);

        // Assign a chit to gen and recompute
        dag.chitconf.insert(gen.hash(), (1, 0)); // Assign chit to gen
        test_recompute(&mut dag, &gen, &[&gen]);
        assert_confidences(&dag, &[(&gen, 5), (&c0, 3), (&c1, 2), (&c2, 1), (&c3, 1)]);

        // Append enough vertices to saturate genesis at the acceptance threshold
        let c4 = test_vertex([&c3]);
        let c5 = test_vertex([&c4]);
        add_to_dag(&mut dag, &c4);
        add_to_dag(&mut dag, &c5);
        dag.chitconf.insert(c4.hash(), (1, 0));
        dag.chitconf.insert(c5.hash(), (1, 0));
        test_recompute(&mut dag, &c4, &[&gen, &c0, &c1, &c2, &c3, &c4]);
        assert_confidences(
            &dag,
            &[
                (&gen, MAX_CONF),
                (&c0, 4),
                (&c1, 3),
                (&c2, 2),
                (&c3, 2),
                (&c4, 1),
            ],
        );
        test_recompute(&mut dag, &c5, &[&gen, &c0, &c1, &c2, &c3, &c4, &c5]);
        assert_confidences(
            &dag,
            &[
                (&gen, MAX_CONF), // should not exceed max
                (&c0, 5),
                (&c1, 5),
                (&c2, 5),
                (&c3, 6),
                (&c4, 6),
                (&c5, 7),
            ],
        );
    }

    #[test]
    fn recompute_preferences() {
        // Set up test vertices and dag
        let gen = Arc::new(Vertex::empty());
        let a00 = test_vertex([&gen]);
        let a01 = test_vertex([&gen]);
        let a02 = test_vertex([&gen]);
        // Create conflicting sets a & b
        let b10 = test_vertex([&a00, &a01]);
        let b11 = test_vertex([&a00, &a02]);
        let c10 = test_vertex([&a01, &a00]);
        let c11 = test_vertex([&a02, &a00]);
        let b20 = test_vertex([&b10]);
        let b21 = test_vertex([&b10, &b11]);
        let c20 = test_vertex([&c10]);
        let c21 = test_vertex([&c10, &c11]);

        // Helper to advance the DAG with new vertices
        let add_to_dag = |dag: &mut DAG, vx: &Arc<Vertex>| {
            dag.vertices.insert(vx.hash(), vx.clone());
            dag.children.insert(vx.hash(), HashMap::new());
            dag.map_child(&vx);
            dag.chitconf.insert(vx.hash(), (0, 0));
            dag.preferences.insert(vx.hash(), false);
            dag.conflicts.insert(&vx);
        };

        // Helper to set the confidence of a vertex
        let set_confidence = |dag: &mut DAG, vx: &Arc<Vertex>, conf: usize| {
            dag.chitconf.get_mut(&vx.hash()).unwrap().1 = conf;
        };

        // Insert everything into the DAG
        let mut dag = DAG::new(Config::default());
        add_to_dag(&mut dag, &gen);
        add_to_dag(&mut dag, &a00);
        add_to_dag(&mut dag, &a01);
        add_to_dag(&mut dag, &a02);
        add_to_dag(&mut dag, &b10);
        add_to_dag(&mut dag, &b11);
        add_to_dag(&mut dag, &b20);
        add_to_dag(&mut dag, &b21);
        add_to_dag(&mut dag, &c10);
        add_to_dag(&mut dag, &c11);
        add_to_dag(&mut dag, &c20);
        add_to_dag(&mut dag, &c21);

        // Basic test
        set_confidence(&mut dag, &gen, 100); // High enough to always be preferred
        assert_eq!(dag.recompute_preference(&gen.hash()), (true, true));
        assert_eq!(dag.recompute_preference(&gen.hash()), (false, true)); // No update

        // Set "b" sub tree to be preferred
        set_confidence(&mut dag, &a00, 10);
        set_confidence(&mut dag, &a01, 10);
        set_confidence(&mut dag, &a02, 10);
        set_confidence(&mut dag, &b10, 5);
        set_confidence(&mut dag, &b11, 4);
        set_confidence(&mut dag, &b20, 3);
        set_confidence(&mut dag, &b21, 3);
        set_confidence(&mut dag, &c10, 0);
        set_confidence(&mut dag, &c11, 4);
        set_confidence(&mut dag, &c20, 10); // cannot be preferred, because of non-preferred parent
        set_confidence(&mut dag, &c21, 10); // cannot be preferred, because of non-preferred parent

        // Check that b subtree is preferred over c
        assert_eq!(dag.recompute_preference(&gen.hash()), (false, true));
        assert_eq!(dag.recompute_preference(&a00.hash()), (true, true));
        assert_eq!(dag.recompute_preference(&a01.hash()), (true, true));
        assert_eq!(dag.recompute_preference(&a02.hash()), (true, true));
        assert_eq!(dag.recompute_preference(&b10.hash()), (true, true));
        assert_eq!(dag.recompute_preference(&b11.hash()), (false, false)); // tie with c11
        set_confidence(&mut dag, &b11, 5);
        assert_eq!(dag.recompute_preference(&b11.hash()), (true, true)); // now beats c11
        assert_eq!(dag.recompute_preference(&b20.hash()), (true, true));
        assert_eq!(dag.recompute_preference(&b21.hash()), (true, true));
        assert_eq!(dag.recompute_preference(&c10.hash()), (false, false));
        assert_eq!(dag.recompute_preference(&c11.hash()), (false, false));
        assert_eq!(dag.recompute_preference(&c20.hash()), (false, false));
        assert_eq!(dag.recompute_preference(&c21.hash()), (false, false));

        // Flip c subtree to being preferred
        set_confidence(&mut dag, &c10, 6);
        set_confidence(&mut dag, &c11, 6);
        assert_eq!(dag.recompute_preference(&gen.hash()), (false, true));
        assert_eq!(dag.recompute_preference(&a00.hash()), (false, true));
        assert_eq!(dag.recompute_preference(&a01.hash()), (false, true));
        assert_eq!(dag.recompute_preference(&a02.hash()), (false, true));
        assert_eq!(dag.recompute_preference(&b10.hash()), (true, false));
        assert_eq!(dag.recompute_preference(&b11.hash()), (true, false));
        assert_eq!(dag.recompute_preference(&b20.hash()), (true, false));
        assert_eq!(dag.recompute_preference(&b21.hash()), (true, false));
        assert_eq!(dag.recompute_preference(&c10.hash()), (true, true));
        assert_eq!(dag.recompute_preference(&c11.hash()), (true, true));
        assert_eq!(dag.recompute_preference(&c20.hash()), (true, true));
        assert_eq!(dag.recompute_preference(&c21.hash()), (true, true));
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
