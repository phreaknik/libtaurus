use super::conflict_set::ConflictSet;
use crate::{Vertex, VertexHash, WireFormat};
use core::panic;
use std::{
    collections::{HashMap, HashSet},
    iter::once,
    result,
    sync::Arc,
};
use tracing::{debug, error, info};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("already exists")]
    AlreadyExists,
    #[error("bad version")]
    BadVersion(u32),
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
    Hash(#[from] crate::hash::Error),
}
type Result<T> = result::Result<T, Error>;

/// Configuraion parameters for a ['DAG'] instance
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

    /// Map of known children for each vertex in the DAG
    children: HashMap<VertexHash, HashMap<VertexHash, Arc<Vertex>>>,

    /// Collection of conflict sets for each vertex in the DAG
    conflict_sets: HashMap<VertexHash, ConflictSet>,

    /// Vertices which define the frontier of the [`DAG`]
    /// The frontier is ordered according to latest ordering preference
    frontier: Vec<VertexHash>,
}

impl DAG {
    /// Create a new [`DAG`] with the given [`Config`]
    pub fn new(config: Config) -> Result<DAG> {
        Ok(DAG {
            config,
            children: HashMap::new(),
            conflict_sets: HashMap::new(),
            frontier: Vec::new(),
        })
    }

    /// Before inserting a vertex into the [`DAG`] it must pass these checks
    fn check_vertex(&self, vx: &Vertex) -> Result<()> {
        // Sanity checks are run in the encoders & decoders to ensure an "insane" vertex never
        // makes it past the wire. Therefore it _should_ be impossible for an insane vertex to make
        // it to the DAG. For performance reasons, we assert this only in debug builds.
        debug_assert!(vx.sanity_checks().is_ok());

        // Make sure a conflict set for this vertex doesn't already exist
        if self.conflict_sets.contains_key(&vx.hash()) {
            return Err(Error::AlreadyExists);
        }

        // Make sure parents exist
        let missing: Vec<_> = vx
            .parents
            .iter()
            .map(|p| p.hash())
            .filter(|h| !self.conflict_sets.contains_key(h))
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(Error::MissingParents(missing))
        }
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

    /// Find any nearby vertex in the [`DAG`]. Useful for searching for conflicts.
    // TODO: should return an Iter type
    fn find_nearby(&self, vx: &Vertex) -> HashSet<VertexHash> {
        let mut nearby = HashSet::new();
        for p in &vx.parents {
            nearby.extend(
                self.children
                    .get(p)
                    .expect("missing parent-child map")
                    .keys(),
            )
        }
        nearby
    }

    /// Finds any [`Vertex`] which conflicts with the given, and registers each with each other's
    /// [`ConflictSet`]. Returns true if the given is preferred amongst all its conflicts.
    fn map_conflicts(&mut self, vx: &Arc<Vertex>) -> bool {
        // Find all conflicts, and register the given [`Vertex`] in their conflict sets. Collect
        // each conflict and its preference.
        let conflicts: Vec<_> = self
            .find_nearby(&vx)
            .iter()
            .filter_map(|v| {
                let cs = self.conflict_sets.get_mut(v).expect("missing conflict set");
                if cs.add_if_conflict(&vx) {
                    Some((cs.owner.clone(), cs.preferred))
                } else {
                    None
                }
            })
            .collect();
        // Add every discovered conflict to the given [`Vertex`]'s [`ConflictSet],
        // and return true if the given vertex is preferred amongst all its conflicts
        let self_conflicts = self
            .conflict_sets
            .get_mut(&vx.hash())
            .expect("missing conflict set");
        conflicts.into_iter().all(|(vx, pref)| {
            self_conflicts.add(&vx);
            pref
        })
    }

    /// Insert a vertex into the [`DAG`]. Returns boolean indicating
    /// if the vertex is preferred or not, as well as a list of known children waiting to be
    /// inserted.
    pub fn insert(&mut self, vx: Arc<Vertex>) -> Result<(bool, Option<Vec<Arc<Vertex>>>)> {
        // Check if the vertex may be inserted
        match self.check_vertex(&vx) {
            res @ Ok(()) | res @ Err(Error::MissingParents(_)) => {
                // Add vertex as known child, even if we are missing some of its parents
                self.map_child(&vx);
                res
            }
            res @ _ => res,
        }?;

        // Create [`ConflictSet`] for this new vertex
        if self
            .conflict_sets
            .insert(vx.hash(), ConflictSet::new(&vx))
            .is_some()
        {
            panic!("conflict set already exists");
        }

        // Add vertex to every conflict set and count how many conflicts were found
        let preferred = self.map_conflicts(&vx);

        // If there were no conflicts, add this vertex to the frontier
        if preferred {
            self.frontier.push(vx.hash());
            self.frontier.extract_if(|v| vx.parents.contains(v)).count();
            self.mark_preferred(&vx.hash())?;
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

    /// Recompute preference and confidences for the given vertex and all its ancestors. Returns
    /// true if any descendent triggered a recomputation.
    fn recompute_at(&mut self, vhash: &VertexHash) -> Result<bool> {
        // Update preference. If parents are preferred and none of [`Vertex`] conflicts are
        // preferred, then it becomes preferred.
        let cs = &self.conflict_sets[vhash];
        let old_preference = cs.preferred;
        let parents_preferred = cs
            .owner
            .parents
            .iter()
            .all(|p| self.conflict_sets[p].preferred);
        let no_conflicts = false;
        let new_preference = parents_preferred && no_conflicts;
        self.conflict_sets
            .get_mut(vhash)
            .ok_or(Error::NotFound)?
            .preferred = new_preference;

        // If preference changed, recurse into children, if any
        let recomputed = if old_preference != new_preference {
            let children: Vec<_> = self.children[vhash].keys().copied().collect();
            // True if any children successfully triggered a recomputation
            children
                .iter()
                .any(|child| self.recompute_at(child).unwrap())
        } else {
            false
        };

        // If preferred, but a confidence recomputation has not been triggered by a descendent,
        // perform it here.
        if !new_preference {
            Ok(false)
        } else {
            if !recomputed {
                // TODO: Recursively compute ancestor confidences
            }
            Ok(true)
        }
    }

    /// Mark the given [`Vertex`] as preferred, updating confidences and the frontier if necessary.
    pub fn mark_preferred(&mut self, vhash: &VertexHash) -> Result<()> {
        let cs = self.conflict_sets.get_mut(vhash).ok_or(Error::NotFound)?;
        if !cs.preferred {
            cs.preferred = true;
            cs.confidence = 1; // Award initial Avalanche chit
        }
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
    fn find_nearby() {
        todo!();
    }

    #[test]
    fn map_conflicts() {
        todo!();
    }

    #[test]
    fn insert() {
        todo!()
    }

    #[test]
    fn recompute_at() {
        todo!()
    }

    #[test]
    fn mark_preferred() {
        todo!()
    }
}
