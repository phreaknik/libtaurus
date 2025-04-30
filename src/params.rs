// TODO: figure out real avalanche parameters

/// If a vertex receives this many consecutive votes, it will be accepted.
pub const AVALANCHE_COUNTER_THRESHOLD: usize = 9;

/// If a vertex reachis this confidence level, it will be accepted.
pub const AVALANCHE_CONFIDENCE_THRESHOLD: usize = 9;

/// Number of positive votes required to satisfy a query
pub const AVALANCHE_QUORUM: usize = 9;

/// Number of peers to query in each Avalanche query round
pub const AVALANCHE_QUERY_COUNT: usize = 16;
