// TODO: figure out real avalanche parameters

use crate::VertexHash;

/// If a vertex receives this many consecutive votes, it will be accepted.
pub const AVALANCHE_COUNTER_THRESHOLD: usize = 9;

/// If a vertex reachis this confidence level, it will be accepted.
pub const AVALANCHE_CONFIDENCE_THRESHOLD: usize = 9;

/// Number of peers to query in each Avalanche query round
pub const AVALANCHE_QUERY_COUNT: usize = 16;

/// The default genesis hash to use when instantiating consensus modules
pub const DEFAULT_GENESIS_HASH: VertexHash = VertexHash::with_bytes([0; 32]);
