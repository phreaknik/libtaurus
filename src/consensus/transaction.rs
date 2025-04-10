use std::result;

use serde_derive::{Deserialize, Serialize};

/// Current version of the transaction structure
pub const VERSION: u32 = 32;

/// Error type for block errors
#[derive(thiserror::Error, Debug)]
pub enum Error {}

/// Result type for block errors
pub type Result<T> = result::Result<T, Error>;

#[derive(Clone, Serialize, Deserialize)]
pub struct Transaction {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct TxHash(pub [u8; blake3::OUT_LEN]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct UtxoHash(pub [u8; blake3::OUT_LEN]);
