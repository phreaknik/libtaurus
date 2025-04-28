use super::conflict_set::ConflictGraph;
use crate::{params, vertex, Vertex, VertexHash, WireFormat};
use itertools::Itertools;
use std::{
    cmp::Ordering,
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
    children: HashMap<VertexHash, HashSet<VertexHash>>,
    progeny: HashMap<VertexHash, HashSet<VertexHash>>,

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
            progeny: HashMap::new(),
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

    /// Add this [`Vertex`] as a known child to each of its parents
    fn map_child(&mut self, vhash: &VertexHash, parent: &VertexHash) {
        if let Some(children) = self.children.get_mut(parent) {
            children.insert(*vhash);
        } else {
            self.children.insert(*parent, once(*vhash).collect());
        }
    }

    /// Add this [`Vertex`] to the progeny of each of its ancestors
    fn map_progeny(&mut self, vhash: &VertexHash, ancestor: &VertexHash) {
        self.progeny.get_mut(ancestor).unwrap().insert(*vhash);
        for parent in &self.vertices[ancestor].parents.clone() {
            self.map_progeny(vhash, parent);
            //TODO: do this in parallel ^
        }
    }

    /// Recursively recompute the state of the given [`Vertex`] and its undecided ancestors
    fn recompute_at(&mut self, vx: &Arc<Vertex>) {
        // Confidence is sum of chits in progeny
        let child_chits = self.progeny[&vx.hash()]
            .iter()
            .filter(|vhash| self.chitconf[vhash].0 != 0)
            .count();
        let new_conf = {
            let chitconf = self.chitconf.get_mut(&vx.hash()).unwrap();
            chitconf.1 = chitconf.0 + child_chits;
            chitconf.1
        };

        // Recurse into parents
        for parent in &self.vertices[&vx.hash()].parents.clone() {
            // TODO: do this in parallel
            self.recompute_at(&self.vertices[parent].clone());
        }

        // Recompute preference
        let conflicts = self.conflicts.conflicts_of(vx);
        let max_conflict_confidence = conflicts
            .iter()
            .map(|vhash| self.chitconf[vhash].1)
            .max()
            .unwrap_or(0);
        let parents_preferred = vx.parents.iter().all(|p| self.preferences[p]);
        let (old_pref, new_pref) = {
            let pref = self.preferences.get_mut(&vx.hash()).unwrap();
            let old_pref = *pref;
            *pref = match new_conf.cmp(&max_conflict_confidence) {
                Ordering::Greater => parents_preferred,
                Ordering::Equal => old_pref && parents_preferred,
                Ordering::Less => false,
            };
            (old_pref, *pref)
        };

        // Helper to recursively reset state for vertex and its progeny
        fn reset(dag: &mut DAG, vhash: VertexHash) {
            // TODO: performance opt: this method should not recurse into children which have
            // already been reset
            *dag.chitconf.get_mut(&vhash).unwrap() = (0, 0);
            *dag.preferences.get_mut(&vhash).unwrap() = false;
            dag.frontier.remove(&vhash); // Remove non-preferred from frontier
            for child in dag.children[&vhash].clone() {
                reset(dag, child)
            }
        }

        // If newly preferred, reset state of each conflict
        if old_pref != new_pref && !old_pref {
            // Reset the state of each conflicting vertex
            for conflict in conflicts {
                reset(self, conflict);
            }

            // Add this vertex to the frontier, if it has no children
            if self.children[&vx.hash()].is_empty() {
                self.frontier.insert(vx.hash());
            }
        }
        println!(
            ":::: recompute_at({}) -> pref={new_pref}, conf={new_conf}, progeny.len()={}, progeny.chits() = {child_chits}",
            vx.hash(),
             self.progeny[&vx.hash()].len(),
        );
    }

    /// Insert a [`Vertex`] without checking. Returns boolean indicating if the vertex is preferred
    /// or not, as well as a list of known children waiting to be inserted.
    fn insert_unchecked(&mut self, vx: &Arc<Vertex>) -> (bool, Option<HashSet<VertexHash>>) {
        // Initialize state variables for this vertex
        let _ = self.children.try_insert(vx.hash(), HashSet::new());
        self.progeny.try_insert(vx.hash(), HashSet::new()).unwrap();
        self.conflicts.insert(vx);
        self.vertices.try_insert(vx.hash(), vx.clone()).unwrap();
        self.chitconf.try_insert(vx.hash(), (0, 0)).unwrap();
        self.preferences.try_insert(vx.hash(), false).unwrap();

        // Update child and progeny maps
        for parent in &vx.parents {
            self.map_child(&vx.hash(), parent);
            self.map_progeny(&vx.hash(), parent);
        }

        // Once child and progeny maps have been updated, recompute state
        self.recompute_at(vx);

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
                for parent in &vx.parents {
                    self.map_child(&vx.hash(), parent);
                }
            }
            e
        })?;

        Ok(self.insert_unchecked(vx))
    }

    /// Return true if the specified vertex is preferred over all its conflicts
    pub fn is_preferred(&self, vx: &Vertex) -> Result<bool> {
        self.preferences
            .get(&vx.hash())
            .copied()
            .ok_or(Error::NotFound)
    }

    /// Award a chit to the specified vertex, according to the Avalanche protocol
    pub fn award_chit(&mut self, vhash: &VertexHash) -> Result<()> {
        self.chitconf.get_mut(vhash).ok_or(Error::NotFound)?.0 = 1;
        self.recompute_at(&self.vertices[vhash].clone());
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
        dag.children.insert(gen.hash(), HashSet::new()); // Genesis is "processed"
        dag.vertices.insert(c0.hash(), c0.clone());
        dag.chitconf.insert(c0.hash(), (0, 0));
        dag.preferences.insert(c0.hash(), false);
        dag.children.insert(c0.hash(), HashSet::new()); // c0 is "processed"
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
        dag.children.insert(c1.hash(), HashSet::new()); // c1 is "processed"
        match dag.check_vertex(&c3) {
            Err(dag::Error::WaitingOnParents(waiting)) => {
                assert_eq!(waiting, [c2.hash()])
            }
            _ => panic!("Expected Error::WaitingOnParents"),
        }
        dag.vertices.insert(c2.hash(), c2.clone());
        dag.chitconf.insert(c2.hash(), (0, 0));
        dag.preferences.insert(c2.hash(), false);
        dag.children.insert(c2.hash(), HashSet::new()); // c2 is "processed"
        assert_matches!(dag.check_vertex(&c3), Ok(()));

        let c4 = test_vertex([&gen, &c1]); // Parents have mismatched height
        assert_matches!(dag.check_vertex(&c4), Err(dag::Error::BadParentHeight));

        dag.vertices.insert(c3.hash(), c3.clone());
        dag.chitconf.insert(c3.hash(), (0, 0));
        dag.preferences.insert(c3.hash(), false);
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
        // Helper to map the child to each of its parents
        let map_child = |dag: &mut DAG, vx: &Arc<Vertex>| {
            for parent in &vx.parents {
                dag.map_child(&vx.hash(), parent)
            }
        };

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
        map_child(&mut dag, &c0);
        assert_eq!(dag.children[&gen.hash()].len(), 1);
        assert_eq!(dag.children[&c0.hash()].len(), 0);
        assert_eq!(dag.children[&c1.hash()].len(), 0);
        assert_eq!(dag.children[&c2.hash()].len(), 0);
        map_child(&mut dag, &c1);
        assert_eq!(dag.children[&gen.hash()].len(), 2);
        assert_eq!(dag.children[&c0.hash()].len(), 0);
        assert_eq!(dag.children[&c1.hash()].len(), 0);
        assert_eq!(dag.children[&c2.hash()].len(), 0);
        map_child(&mut dag, &c2);
        assert_eq!(dag.children[&gen.hash()].len(), 2);
        assert_eq!(dag.children[&c0.hash()].len(), 1);
        assert_eq!(dag.children[&c1.hash()].len(), 1);
        assert_eq!(dag.children[&c2.hash()].len(), 0);
    }

    #[test]
    fn map_progeny() {
        todo!()
    }

    #[test]
    fn recompute_at() {
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
        let c10 = test_vertex([&a01, &a00]);
        let c11 = test_vertex([&a02, &a00]);
        let c20 = test_vertex([&c10]);
        let c21 = test_vertex([&c10, &c11]);
        let c30 = test_vertex([&c21, &c20]);

        // Helper to advance the DAG with new vertices
        let add_to_dag = |dag: &mut DAG, vx: &Arc<Vertex>| {
            dag.vertices.insert(vx.hash(), vx.clone());
            dag.children.insert(vx.hash(), HashSet::new());
            dag.progeny.insert(vx.hash(), HashSet::new());
            dag.chitconf.insert(vx.hash(), (0, 0));
            dag.preferences.insert(vx.hash(), false);
            dag.conflicts.insert(&vx);
            for parent in &vx.parents {
                dag.map_child(&vx.hash(), parent);
                dag.map_progeny(&vx.hash(), parent);
            }
        };

        // Helper to assert that all confidences match expected
        let assert_confs_and_prefs = |dag: &DAG, expected: &[(&Arc<Vertex>, (usize, bool))]| {
            // Print actual states
            expected
                .iter()
                .map(|(vx, _)| {
                    (
                        vx.hash(),
                        dag.chitconf[&vx.hash()].0,
                        dag.chitconf[&vx.hash()].1,
                        dag.preferences[&vx.hash()],
                    )
                })
                .for_each(|(vhash, chit, conf, pref)| {
                    println!("state({vhash}) = ({chit}, {conf}, {pref})");
                });
            // Confirm actual states match expected states
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
            if chit == 0 {
                // If we are resetting this vertex, also clear its confidence and preference
                dag.chitconf.get_mut(&vx.hash()).unwrap().1 = 0;
                *dag.preferences.get_mut(&vx.hash()).unwrap() = false;
            }
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
        dag.recompute_at(&gen);
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
        println!(":::: b30 = {}", &b30.hash());
        dag.recompute_at(&b30);
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

        // Awarding chits to "c" sub tree instead
        set_chit(&mut dag, &b10, 0);
        set_chit(&mut dag, &b11, 0);
        set_chit(&mut dag, &b20, 0);
        set_chit(&mut dag, &b21, 0);
        set_chit(&mut dag, &b30, 0);
        set_chit(&mut dag, &c10, 1);
        set_chit(&mut dag, &c11, 1);
        set_chit(&mut dag, &c20, 1);
        set_chit(&mut dag, &c21, 1);
        set_chit(&mut dag, &c30, 1);
        println!(":::: c30 = {}", &c30.hash());
        dag.recompute_at(&c30);
        assert_confs_and_prefs(
            &dag,
            &[
                (&gen, (9, true)),
                (&a00, (6, true)),
                (&a01, (5, true)),
                (&a02, (4, true)),
                (&b10, (0, false)),
                (&b11, (0, false)),
                (&b20, (0, false)),
                (&b21, (0, false)),
                (&b30, (0, false)),
                (&c10, (4, true)),
                (&c11, (3, true)),
                (&c20, (2, true)),
                (&c21, (2, true)),
                (&c30, (1, true)),
            ],
        );

        // Extend progeny of b30 to flip "b" sub tree back into preference
        let b40 = test_vertex([&b30]);
        let b50 = test_vertex([&b40]);
        let b60 = test_vertex([&b50]);
        let b70 = test_vertex([&b60]);
        let b80 = test_vertex([&b70]);
        add_to_dag(&mut dag, &b40);
        add_to_dag(&mut dag, &b50);
        add_to_dag(&mut dag, &b60);
        add_to_dag(&mut dag, &b70);
        add_to_dag(&mut dag, &b80);
        set_chit(&mut dag, &b40, 1);
        set_chit(&mut dag, &b50, 1);
        set_chit(&mut dag, &b60, 1);
        set_chit(&mut dag, &b70, 1);
        set_chit(&mut dag, &b80, 1);
        println!(":::: b80 = {}", &b80.hash());
        dag.recompute_at(&b80);
        println!(
            ":::: gen.progeny = len={}, w/chits={}",
            dag.progeny[&gen.hash()].len(),
            dag.progeny[&gen.hash()]
                .iter()
                .filter(|vhash| dag.chitconf[vhash].0 != 0)
                .count()
        );
        println!(
            ":::: a00.progeny = len={}, w/chits={}",
            dag.progeny[&a00.hash()].len(),
            dag.progeny[&a00.hash()]
                .iter()
                .filter(|vhash| dag.chitconf[vhash].0 != 0)
                .count()
        );
        println!(
            ":::: a01.progeny = len={}, w/chits={}",
            dag.progeny[&a01.hash()].len(),
            dag.progeny[&a01.hash()]
                .iter()
                .filter(|vhash| dag.chitconf[vhash].0 != 0)
                .count()
        );
        println!(
            ":::: a02.progeny = len={}, w/chits={}",
            dag.progeny[&a02.hash()].len(),
            dag.progeny[&a02.hash()]
                .iter()
                .filter(|vhash| dag.chitconf[vhash].0 != 0)
                .count()
        );
        assert_confs_and_prefs(
            &dag,
            &[
                (&gen, (MAX_CONF, true)),
                (&a00, (6, true)),
                (&a01, (6, true)),
                (&a02, (6, true)),
                (&b10, (5, true)),
                (&b11, (5, true)),
                (&b20, (5, true)),
                (&b21, (5, true)),
                (&b30, (5, true)),
                (&c10, (0, false)),
                (&c11, (0, false)),
                (&c20, (0, false)),
                (&c21, (0, false)),
                (&c30, (0, false)),
                (&b40, (5, true)),
                (&b50, (4, true)),
                (&b60, (3, true)),
                (&b70, (2, true)),
                (&b80, (1, true)),
            ],
        );
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
    fn award_chit() {
        todo!()
    }

    #[test]
    fn get_frontier() {
        todo!()
    }
}
