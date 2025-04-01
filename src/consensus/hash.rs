use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Hash([u8; blake3::OUT_LEN]);

impl From<blake3::Hash> for Hash {
    fn from(value: blake3::Hash) -> Self {
        Hash(*value.as_bytes())
    }
}
