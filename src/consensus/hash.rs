use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Hash([u8; blake3::OUT_LEN]);

impl Hash {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl From<blake3::Hash> for Hash {
    fn from(value: blake3::Hash) -> Self {
        Hash(*value.as_bytes())
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{:x}", byte)?;
        }
        Ok(())
    }
}
