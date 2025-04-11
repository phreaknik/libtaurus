use super::ewma::EWMA;
use chrono::Duration;
use std::cmp::max;

const EWMA_ALPHA: f64 = 0.1;

/// Statistics about a specific miner on the network
pub struct MinerStats {
    hash_rate: EWMA<f64>,
}

impl MinerStats {
    /// Create a new stats object
    pub fn new() -> MinerStats {
        MinerStats {
            hash_rate: EWMA::new(0.0, EWMA_ALPHA),
        }
    }

    /// Update miner statistics for a mined block
    pub fn mined_block(&mut self, difficulty: f64, duration: Duration) {
        let seconds = max(1, duration.num_seconds());
        let inst_hr = difficulty / (seconds as f64);
        self.hash_rate.insert(inst_hr);
    }
}
