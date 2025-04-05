pub mod dag;
pub mod vertex;

use crate::randomx;
pub use dag::{Frontier, DAG};
use std::result;
pub use vertex::{CompactVertex, Vertex};

/// Error type for Avalanche errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Cbor(#[from] serde_cbor::Error),
    #[error(transparent)]
    RandomX(#[from] randomx::Error),
    #[error("invalid difficulty")]
    InvalidDifficulty,
    #[error("invalid proof-of-work")]
    InvalidPoW,
    #[error("missing parent")]
    MissingParent,
    #[error("error acquiring read lock")]
    ReadLock,
    #[error("error acquiring write lock")]
    WriteLock,
}

/// Result type for consensus errors
pub type Result<T> = result::Result<T, Error>;
