use crate::{params, Vertex, VertexHash, WireFormat};
use std::{collections::HashMap, iter::once, result, sync::Arc};
use tracing::{debug, error, info};

use super::conflict_set::ConflictGraph;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("already exists")]
    AlreadyExists,
    #[error("bad version")]
    BadVersion(u32),
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
    #[error("waiting on parents to process")]
    WaitingOnParents(Vec<VertexHash>),
}
type Result<T> = result::Result<T, Error>;

/// Configuraion parameters for a [`DAG`] instance
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct Config {}

impl Config {
    pub fn new() -> Config {
        Config {}
    }
}

/// Implementation of SESAME DAG
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct DAG {
    /// Configuration parameters
    config: Config,

    /// Map of every [`Vertex`] in the [`DAG`]
    vertices: HashMap<VertexHash, Arc<Vertex>>,

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
    frontier: Vec<VertexHash>,
}

impl DAG {
    /// Create a new [`DAG`] with the given [`Config`]
    pub fn new(config: Config) -> Result<DAG> {
        Ok(DAG {
            config,
            vertices: HashMap::new(),
            chitconf: HashMap::new(),
            preferences: HashMap::new(),
            children: HashMap::new(),
            conflicts: ConflictGraph::new(),
            frontier: Vec::new(),
        })
    }

    /// Before inserting a vertex into the [`DAG`] it must pass these checks
    fn check_vertex(&self, vx: &Vertex) -> Result<()> {
        // Sanity checks are run in the encoders & decoders to ensure an "insane" vertex never
        // makes it past the wire. Therefore it _should_ be impossible for an insane vertex to make
        // it to the DAG. For performance reasons, we assert this only in debug builds.
        debug_assert!(vx.sanity_checks().is_ok());

        // Check the child map to make sure this vertex doesn't already exist
        if self.children.contains_key(&vx.hash()) {
            return Err(Error::AlreadyExists);
        }

        // Make sure parents exist
        let missing: Vec<_> = vx
            .parents
            .iter()
            .map(|p| p.hash())
            .filter(|h| !self.children.contains_key(h))
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
            .map(|p| p.hash())
            .filter(|h| !self.chitconf.contains_key(h))
            .collect();
        if !missing.is_empty() {
            return Err(Error::WaitingOnParents(waiting));
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
        if let Some(changes) = self.recompute_confidences(vhash)? {
            for changed in &changes {
                // TODO: what if an error causes partial state changes? e.g. some states changed,
                // but not others?
                self.recompute_preference(changed)?;
            }
        }
        Ok(())
    }

    /// Recursively recompute the confidences of the given [`Vertex`], and each of its undecided
    /// ancestors, returning the hashes of every vertex which changed, ordered from oldest to
    /// youngest
    fn recompute_confidences(&mut self, vhash: &VertexHash) -> Result<Option<Vec<VertexHash>>> {
        // Compute the new confidence value
        let progeny_confidence: usize = self.children[vhash]
            .keys()
            .map(|child| self.chitconf[child].1)
            .sum();

        // Update the confidence
        let chitconf = self.chitconf.get_mut(vhash).ok_or(Error::NotFound)?;
        let new_confidence = usize::min(
            chitconf.0 + progeny_confidence, // confidence(v) = v.chit + confidence(v.progeny)
            params::AVALANCHE_ACCEPTANCE_THRESHOLD,
        );
        let changed = chitconf.1 != new_confidence;
        chitconf.1 = new_confidence;

        // If the state changed, recurse into children
        // TODO: do this in parallel
        if changed {
            Ok(Some(
                self.vertices[vhash]
                    .clone()
                    .parents
                    .iter()
                    .filter_map(|parent| self.recompute_confidences(&parent).unwrap())
                    .flatten()
                    .collect(),
            ))
        } else {
            Ok(None)
        }
    }

    /// Recursively recompute the preference of the given [`Vertex`], and each of its children
    fn recompute_preference(&mut self, vhash: &VertexHash) -> Result<()> {
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
        if old_preference != new_preference {
            self.preferences.insert(*vhash, new_preference);

            // If this vertex just became preferred, reset all conflicts
            if new_preference {
                for &&vhash in conflicts.keys() {
                    self.preferences.insert(vhash, false);
                    self.chitconf.insert(vhash, (0, 0));
                }
            }
            // TODO: what if flip from preferred?
        }

        Ok(())
    }

    /// Insert a vertex into the [`DAG`]. Returns boolean indicating
    /// if the vertex is preferred or not, as well as a list of known children waiting to be
    /// inserted.
    pub fn insert(&mut self, vx: &Arc<Vertex>) -> Result<(bool, Option<Vec<Arc<Vertex>>>)> {
        // Check if the vertex may be inserted
        match self.check_vertex(vx) {
            res @ Ok(()) | res @ Err(Error::MissingParents(_)) => {
                // Add vertex as known child, even if we are missing some of its parents
                self.map_child(vx);
                res
            }
            res @ _ => res,
        }?;

        // Initialize vertex state variables
        self.conflicts.insert(vx);
        self.vertices.insert(vx.hash(), vx.clone());
        self.chitconf.insert(vx.hash(), (0, 0));
        self.preferences.insert(vx.hash(), false);

        // Recompute states
        self.recompute_at(&vx.hash())?;

        let preferred = self.is_preferred(vx)?;
        if preferred {
            // Add this vertex to the frontier
            self.frontier.extract_if(|v| vx.parents.contains(v)).count();
            self.frontier.push(vx.hash());
            // TODO: this should happen as part of recompute
        }

        error!("these prints should move to taurusd");
        debug!("Vertex {} = {}", vx.hash(), vx);
        info!("Vertex {} inserted", vx.hash());
        Ok((
            preferred,
            self.children
                .get(&vx.hash())
                .and_then(|c| Some(c.values().cloned().collect())),
        ))
    }

    /// Award a chit to the specified vertex, according to the Avalanche protocol
    pub fn award_chit(&mut self, vhash: &VertexHash) -> Result<()> {
        self.chitconf.get_mut(vhash).ok_or(Error::NotFound)?.0 = 1;
        self.recompute_at(vhash)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn new_config() {
        todo!()
    }

    #[test]
    fn new_dag() {
        todo!()
    }

    #[test]
    fn check_vertex() {
        todo!();
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
    fn insert() {
        todo!()
    }

    #[test]
    fn award_chit() {
        todo!()
    }
}
