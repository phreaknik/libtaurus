use crate::{Vertex, VertexHash, WireFormat};
use std::{collections::HashMap, iter::once, result, sync::Arc};
use tracing::{debug, error, info};

use super::conflict_set::ConflictGraph;

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

    /// Insert a vertex into the [`DAG`]. Returns boolean indicating
    /// if the vertex is preferred or not, as well as a list of known children waiting to be
    /// inserted.
    pub fn insert(&mut self, vx: Arc<Vertex>) -> Result<(bool, Option<Vec<Arc<Vertex>>>)> {
        // Check if the vertex may be inserted
        match self.check_vertex(&vx) {
            res @ Ok(()) | res @ Err(Error::MissingParents(_)) => {
                // Add vertex as known child, even if we are missing some of its parents
                self.map_child(&vx);
                // Add vertex to conflict graph
                self.conflicts.insert(&vx);
                res
            }
            res @ _ => res,
        }?;

        // TODO: do a real preference calc
        let preferred = true;

        // Add this vertex to the frontier
        self.frontier.push(vx.hash());
        self.frontier.extract_if(|v| vx.parents.contains(v)).count();

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
    pub fn award_chit(&mut self, _vhash: &VertexHash) {
        todo!()
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
    fn insert() {
        todo!()
    }

    #[test]
    fn award_chit() {
        todo!()
    }
}
