use crate::Vertex;
use std::result;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("bad version")]
    BadVersion(u32),
    #[error("no parents")]
    NoParents,
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
pub struct DAG {}

impl DAG {
    /// Create a new ['DAG'] with the given ['Config']
    pub fn new(_config: Config) -> Result<DAG> {
        Ok(DAG {})
    }

    /// Insert a vertex into the DAG
    pub fn insert(&mut self, _vertex: Vertex) -> Result<()> {
        todo!()
    }

    /// Look up the frontier vertices in the DAG
    pub fn get_frontier(self) -> Result<Vec<Vertex>> {
        todo!()
    }
}
