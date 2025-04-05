pub mod vertex;

use crate::randomx;
use std::result;

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
}

/// Result type for consensus errors
pub type Result<T> = result::Result<T, Error>;
