#![feature(assert_matches)]
#![feature(hash_extract_if)]
#![feature(iterator_try_collect)]
#![feature(iterator_try_reduce)]
#![feature(result_flattening)]
#![feature(map_try_insert)]
// TODO: reasses these unstable features

pub mod consensus;
pub mod hash;
pub mod p2p;
pub mod params;
pub mod rpc;
pub mod util;
pub mod wire;

pub use consensus::{vertex, GenesisConfig, Vertex, VertexHash};
pub use wire::WireFormat;
