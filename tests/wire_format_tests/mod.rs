mod block;
mod consensus_requests;
mod hash;
mod p2p_broadcasts;
mod transaction;
mod vertex;

use cordelia::WireFormat;
use quick_protobuf::{MessageRead, MessageWrite};
use std::fmt::Debug;

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
    P: MessageWrite + MessageRead<'a>,
    D: WireFormat<'a, P> + Debug + Eq,
{
    let encode_err = match decoded.to_wire(true) {
        Ok(bytes) => {
            assert_eq!(bytes, encoded);
            None
        }
        Err(e) => Some(e),
    };
    let decode_err = match <D as WireFormat<'a, P>>::from_wire(&encoded, true) {
        Ok(block) => {
            assert_eq!(block, decoded);
            None
        }
        Err(e) => Some(e),
    };
    (encode_err, decode_err)
}
