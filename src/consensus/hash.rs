use std::fmt;

use heed::{BytesDecode, BytesEncode};
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

impl<'a> BytesEncode<'a> for Hash {
    type EItem = Hash;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(serde_cbor::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for Hash {
    type DItem = Hash;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(serde_cbor::from_slice(bytes)?)
    }
}
