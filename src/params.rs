use chrono::{DateTime, Utc};

/// Revision number of the consensus protocol
pub const PROTOCOL_VERSION: u32 = 0;

/// Minimum block difficulty
pub const MIN_DIFFICULTY: u64 = 1_000;

/// Difficulty of the genesis block
pub const GENESIS_DIFFICULTY: u64 = MIN_DIFFICULTY;

/// Timestamp of the genesis block
pub const GENESIS_TIMESTAMP: DateTime<Utc> = DateTime::<Utc>::MIN_UTC;

/// Number of validators to be sampled in an Avalanche query
pub const VALIDATOR_QUORUM_SIZE: usize = 23;

/// Age after which a raffle ticket is no longer considered for consensus
pub const RAFFLE_TICKET_MAX_AGE_DAYS: i64 = 7;
