use chrono::{DateTime, Utc};

pub const MIN_DIFFICULTY: u64 = 1_000;
pub const GENESIS_DIFFICULTY: u64 = MIN_DIFFICULTY;
pub const GENESIS_TIMESTAMP: DateTime<Utc> = DateTime::<Utc>::MIN_UTC;
pub const VALIDATOR_QUORUM_SIZE: usize = 23;
pub const RAFFLE_TICKET_MAX_AGE_DAYS: i64 = 7;
