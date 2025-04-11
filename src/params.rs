use chrono::{DateTime, Utc};

/// Minimum block difficulty
pub const MIN_DIFFICULTY: u64 = 1_000;

/// Difficulty of the genesis block
pub const GENESIS_DIFFICULTY: u64 = MIN_DIFFICULTY;

/// Timestamp of the genesis block
pub const GENESIS_TIMESTAMP: DateTime<Utc> = DateTime::<Utc>::MIN_UTC;

/// Number of peers to query for block preference, according to Avalanche consensus.
pub const AVALANCHE_QUERY_COUNT: usize = 16;

/// Number of peers which must prefer the block to earn a chit, according to Avalanche consensus.
// TODO: figure out real avalanche parameters
pub const AVALANCHE_QUORUM: usize = 9;

/// Confidence level a vertex must achieve to become accepted
pub const AVALANCHE_ACCEPTANCE_THRESHOLD: usize = 9;

/// Time we allow for each peer in a query to respond
pub const QUERY_TIMEOUT_SEC: u64 = 10;
