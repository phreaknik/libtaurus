pub mod generated {
    pub mod proto {
        include!(concat!(env!("OUT_DIR"), "/generated.proto.rs"));
    }
}

use crate::hash::Hash;
pub use generated::proto;
use std::{
    fmt::{Debug, Display},
    result,
};

/// Error type for vertex errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    ProstDecode(#[from] prost::DecodeError),
    #[error(transparent)]
    ProstEncode(#[from] prost::EncodeError),
    #[error(transparent)]
    Hash(#[from] crate::hash::Error),
}

pub trait WireFormat<'a, P>: Sized
where
    P: prost::Message + Default,
{
    type Error: Debug + From<prost::DecodeError> + From<prost::EncodeError> + Display;

    /// Serialize into protobuf format
    fn to_protobuf(&self, check: bool) -> result::Result<P, Self::Error>;

    /// Deserialize from protobuf format
    fn from_protobuf(data: &P, check: bool) -> result::Result<Self, Self::Error>;

    /// Serialize data into a format suitable for wire transmission, optionally checking validity
    /// of the data before serialization.
    fn to_wire(&self, check: bool) -> result::Result<Vec<u8>, Self::Error> {
        let mut bytes = Vec::new();
        let protobuf = self.to_protobuf(check)?;
        protobuf.encode(&mut bytes)?;
        Ok(bytes)
    }

    /// Deserialize data from wire format, optionally checking validity of the data before
    /// deserialization.
    fn from_wire(bytes: &'a [u8], check: bool) -> result::Result<Self, Self::Error> {
        let protobuf = prost::Message::decode(bytes)?;
        Self::from_protobuf(&protobuf, check)
    }

    /// Compute hash of the serialized data
    fn hash(&self) -> Hash {
        blake3::hash(&self.to_wire(false).expect("Error encoding to wire format")).into()
    }
}

/// Tests that data matches after encoding the object.
pub fn test_wire_encode<'a, P, D>(
    decoded: D,
    encoded: &'a [u8],
) -> Option<<D as WireFormat<'a, P>>::Error>
where
    P: prost::Message + Default,
    D: WireFormat<'a, P> + Debug + Eq,
{
    let err = match decoded.to_wire(true) {
        Ok(bytes) => {
            assert_eq!(bytes, encoded);
            None
        }
        Err(e) => Some(e),
    };
    err
}

/// Tests that the object matches after decoding the data.
pub fn test_wire_decode<'a, P, D>(
    decoded: D,
    encoded: &'a [u8],
) -> Option<<D as WireFormat<'a, P>>::Error>
where
    P: prost::Message + Default,
    D: WireFormat<'a, P> + Debug + Eq,
{
    let err = match <D as WireFormat<'a, P>>::from_wire(&encoded, true) {
        Ok(object) => {
            assert_eq!(object, decoded);
            None
        }
        Err(e) => Some(e),
    };
    err
}

/// Tests that encoded data matches decoded after encoding and decoding each. Returns the
/// encode and decode errors, if any.
pub fn test_wire_format<'a, P, D>(
    decoded: D,
    encoded: &'a [u8],
) -> (
    Option<<D as WireFormat<'a, P>>::Error>,
    Option<<D as WireFormat<'a, P>>::Error>,
)
where
    P: prost::Message + Default,
    D: WireFormat<'a, P> + Debug + Eq + Clone,
{
    (
        test_wire_encode(decoded.clone(), encoded),
        test_wire_decode(decoded, encoded),
    )
}
