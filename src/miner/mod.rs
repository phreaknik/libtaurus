use crate::{consensus, util, Frontier, SlimFrontier};
use num::{BigUint, FromPrimitive};
use std::result;
use std::time::Duration;
use tokio::select;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::spawn_blocking;
use tokio::time::interval;
use tracing::{debug, error, info};
use tracing_log::log::warn;

/// Event channel capacity. Old events will be dropped if channel exceeds capacity. See
/// [`tokio::sync::broadcast`] for more information.
const MINER_EVENT_CHAN_CAPACITY: usize = 32;

/// Local difficulty for reporting mining shares
pub const MINING_SHARE_DIFFICULTY: u64 = 1000_000;

/// Period to print miner stats
const STATS_PERIOD_SECOND: Duration = Duration::from_secs(60);

/// Event produced by the miner
#[derive(Debug, Clone)]
pub enum Event {}

/// Actions that can be performed by the miner
#[derive(Clone, Debug)]
pub enum Action {}

/// Error type for mining errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Dial(#[from] consensus::Error),
    #[error(transparent)]
    MinerChanClosed(#[from] mpsc::error::SendError<SlimFrontier>),
}

/// Result type for mining errors
pub type Result<T> = result::Result<T, Error>;

/// Configuration details for the mining process.
#[derive(Debug, Clone)]
pub struct Config {
    /// Number of mining threads to spawn. If unspecified, will use max available CPUs.
    pub num_threads: Option<usize>,
}

/// Run the mining process, spawning the task as a new thread. Returns an ['broadcast::Sender'],
/// which can be subscribed to, to receive mining events from the task.
pub fn start(
    config: Config,
    consensus_action_ch: UnboundedSender<consensus::Action>,
    consensus_event_ch: broadcast::Receiver<consensus::Event>,
) -> (UnboundedSender<Action>, broadcast::Sender<Event>) {
    let (action_sender, action_receiver) = mpsc::unbounded_channel();
    let (event_sender, _) = broadcast::channel(MINER_EVENT_CHAN_CAPACITY);
    tokio::spawn(task_fn(
        config,
        action_receiver,
        event_sender.clone(),
        consensus_action_ch,
        consensus_event_ch,
    ));
    (action_sender, event_sender)
}

/// The task function which runs the mining process.
async fn task_fn(
    config: Config,
    mut _actions_in: UnboundedReceiver<Action>,
    mut _events_out: broadcast::Sender<Event>,
    consensus_action_ch: UnboundedSender<consensus::Action>,
    mut consensus_event_ch: broadcast::Receiver<consensus::Event>,
) {
    info!("Starting miner...");
    let (_, mut results_receiver) = mpsc::unbounded_channel();
    let sols_count_sender = start_stats();
    loop {
        select! {
            // Handle consensus events
            event = consensus_event_ch.recv() => {
                match event {
                    Err(channel_error) => error!("consensus_event_ch error: {channel_error}"),
                    Ok(event) => {
                        match event {
                            // Restart mining threads to mine on new frontier
                            consensus::Event::NewFrontier(frontier) => {
                                results_receiver.close(); // Kill previous mining threads
                                results_receiver = match spawn_mining_threads(config.num_threads.unwrap_or(0), frontier, sols_count_sender.clone()) {
                                    Ok(ch) => ch,
                                    Err(e) => {
                                        error!("failed to start miners: {e}");
                                        continue;
                                    },
                                }
                            }
                        }
                    },
                }
            }

            // Handle results from the mining threads
            Some(result) = results_receiver.recv() => {
                if result.verify_pow().is_ok() {
                    info!("found proof-of-work: {}", result.hash());
                    if consensus_action_ch.send(consensus::Action::SubmitMinedFrontier(result)).is_err() {
                        error!("stopping...");
                    }
                } else {
                    debug!("found mining share");
                }
            }
        }
    }
}

/// Spawn mining threads for the given work
fn spawn_mining_threads(
    num_threads: usize,
    frontier: Frontier,
    sols_count_ch: UnboundedSender<usize>,
) -> Result<UnboundedReceiver<SlimFrontier>> {
    info!("mining on new frontier: {}", frontier.hash());
    // Close the old channel to kill the old mining threads, and
    // create a new channel for the new mining threads.
    let (results_sender, results_receiver) = mpsc::unbounded_channel();
    let slim: SlimFrontier = frontier.try_into()?;
    let target = BigUint::from_u64(2).unwrap().pow(256)
        / BigUint::from_u64(MINING_SHARE_DIFFICULTY).unwrap();
    for _ in 0..num_threads {
        let slim = slim.clone();
        let target = target.clone();
        let results_sender = results_sender.clone();
        let sols_count_ch = sols_count_ch.clone();
        spawn_blocking(|| match mine(slim, target, results_sender, sols_count_ch) {
            Ok(_) | Err(Error::MinerChanClosed(_)) => {}
            Err(e) => {
                warn!("mining thread stopped with error: {e}");
            }
        });
    }
    Ok(results_receiver)
}

/// Find a nonce which satisfies the difficulty target
fn mine(
    mut frontier: SlimFrontier,
    target: BigUint,
    results_ch: UnboundedSender<SlimFrontier>,
    sols_count_ch: UnboundedSender<usize>,
) -> Result<()> {
    let mut hasher = blake3::Hasher::new();
    frontier.nonce = rand::random();
    loop {
        let loop_count = 1_000_000_000;
        for i in 0..loop_count {
            hasher.update(&serde_cbor::to_vec(&frontier).unwrap());
            if BigUint::from_bytes_be(hasher.finalize().as_bytes()) < target {
                results_ch.send(frontier.clone())?;
                sols_count_ch.send(i).unwrap();
            } else if results_ch.is_closed() {
                return Ok(());
            }
            hasher.reset();
            frontier.nonce += 1;
        }
        sols_count_ch.send(loop_count).unwrap();
    }
}

/// Start the mining stats thread
pub fn start_stats() -> UnboundedSender<usize> {
    let (solutions_sender, solutions_receiver) = mpsc::unbounded_channel();
    tokio::spawn(stats_fn(solutions_receiver));
    solutions_sender
}

/// The task function which runs the mining process.
async fn stats_fn(mut solutions_receiver: UnboundedReceiver<usize>) {
    let mut count = 0u64;
    let mut ticker = interval(STATS_PERIOD_SECOND);
    loop {
        select! {
            // accumulate solutions
            event = solutions_receiver.recv() => {
                match event {
                    Some(sols_count) => count += sols_count as u64,
                    None => return, // channel closed. exit now.
                }
            }

            // print stats
            _ = ticker.tick() => {
                let rate = count/ STATS_PERIOD_SECOND.as_secs();
                count = 0;
                info!("current hashrate: {}", util::human_readable(rate, "h/s"));
            }
        }
    }
}
