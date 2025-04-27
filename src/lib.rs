#![feature(assert_matches)]
#![feature(hash_extract_if)]
#![feature(iterator_try_collect)]
#![feature(result_flattening)]

pub mod consensus;
pub mod hash;
pub mod http;
pub mod p2p;
pub mod params;
pub mod util;
pub mod wire;

pub use consensus::{vertex, GenesisConfig, Vertex, VertexHash};
pub use wire::WireFormat;
