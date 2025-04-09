use chrono::{DateTime, Utc};

/// Revision number of the consensus protocol
pub const PROTOCOL_VERSION: u32 = 0;

/// Minimum block difficulty
pub const MIN_DIFFICULTY: u64 = 1_000;

/// Difficulty of the genesis block
pub const GENESIS_DIFFICULTY: u64 = MIN_DIFFICULTY;

/// Timestamp of the genesis block
pub const GENESIS_TIMESTAMP: DateTime<Utc> = DateTime::<Utc>::MIN_UTC;

/// Number of peers to query for block preference, according to Avalanche.
pub const QUORUM_SIZE: usize = 16;

// TODO: Select appropriate quorum size and age
/// Number of blocks to select quorum
pub const QUORUM_MINER_AGE: usize = 1024;
