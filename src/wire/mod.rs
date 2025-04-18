use crate::hash::Hash;
use std::fmt::Debug;

pub trait WireFormat: Sized {
    type Error: Debug;

    /// Serialize data into a format suitable for wire transmission
    fn to_wire(&self) -> Result<Vec<u8>, Self::Error>;

    /// Deserialize data from wire format
    fn from_wire(bytes: &[u8]) -> Result<Self, Self::Error>;

    /// Compute hash of the serialized data
    fn hash(&self) -> Hash {
        blake3::hash(&self.to_wire().expect("Error encoding to wire format")).into()
    }
}
