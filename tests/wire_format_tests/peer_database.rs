use taurus::{p2p::PeerInfo, wire::test_wire_format};
use libp2p::multiaddr::Multiaddr;
use std::assert_matches::assert_matches;

#[test]
pub fn basic_peer_info() {
    let decoded = PeerInfo {
        protocol_version: "myproto".to_string(),
        agent_version: "myagent".to_string(),
        addresses: vec![Multiaddr::empty(), "/ip4/127.0.0.1".parse().unwrap()],
    };
    let encoded = &[
        10, 7, 109, 121, 112, 114, 111, 116, 111, 18, 7, 109, 121, 97, 103, 101, 110, 116, 26, 0,
        26, 14, 47, 105, 112, 52, 47, 49, 50, 55, 46, 48, 46, 48, 46, 49,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, None);
    assert_matches!(decode_err, None);
}
