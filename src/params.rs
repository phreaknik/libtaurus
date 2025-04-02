use chrono::{DateTime, Utc};

pub const MIN_DIFFICULTY: u64 = 1_000;
pub const GENESIS_DIFFICULTY: u64 = MIN_DIFFICULTY;
pub const GENESIS_TIMESTAMP: DateTime<Utc> = DateTime::<Utc>::MIN_UTC;
