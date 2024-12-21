pub mod generated {
    pub mod proto {
        include!(concat!(env!("OUT_DIR"), "/wire.pb.rs"));
    }
}

use crate::hash::Hash;
pub use generated::proto;
use std::{
    fmt::{Debug, Display},
    result,
};

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

#[cfg(test)]
mod test {
    use super::Hash;

    #[test]
    fn corner_cases() {
        pub struct HashTestCase<'a> {
            pub decoded: Hash,
            pub long_hex: &'a str,
            pub short_hex: &'a str,
        }
        for test in [
            HashTestCase {
                decoded: Hash::with_bytes([
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0,
                ]),
                long_hex: "0000000000000000000000000000000000000000000000000000000000000000",
                short_hex: "00000000..",
            },
            HashTestCase {
                decoded: Hash::with_bytes([
                    128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0,
                ]),
                long_hex: "8000000000000000000000000000000000000000000000000000000000000000",
                short_hex: "80000000..",
            },
            HashTestCase {
                decoded: Hash::with_bytes([
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 1,
                ]),
                long_hex: "0000000000000000000000000000000000000000000000000000000000000001",
                short_hex: "00000000..",
            },
            HashTestCase {
                decoded: Hash::with_bytes([
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                ]),
                long_hex: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                short_hex: "ffffffff..",
            },
            HashTestCase {
                decoded: Hash::with_bytes([
                    127, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                ]),
                long_hex: "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                short_hex: "7fffffff..",
            },
            HashTestCase {
                decoded: Hash::with_bytes([
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254,
                ]),
                long_hex: "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe",
                short_hex: "ffffffff..",
            },
        ] {
            assert_eq!(test.decoded.to_hex(), test.long_hex);
            assert_eq!(test.decoded.to_short_hex(), test.short_hex);
        }
    }
}
