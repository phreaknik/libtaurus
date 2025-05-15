#![feature(assert_matches)]
#![feature(hash_extract_if)]
#![feature(iterator_try_collect)]
#![feature(iterator_try_reduce)]
#![feature(result_flattening)]
#![feature(map_try_insert)]
// TODO: reasses these unstable features

pub mod app;
pub mod consensus;
pub mod fetcher;
pub mod hash;
pub mod p2p;
pub mod rpc;
pub mod sequencer;
pub mod util;
pub mod wire;

pub use consensus::{dag, vertex, GenesisConfig, Vertex, VertexHash};
pub use wire::WireFormat;
