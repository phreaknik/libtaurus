use super::database::ValidatorDatabase;
use crate::consensus::validators::database::ValidatorDbKey;
use crate::consensus::Config;
use crate::params::{RAFFLE_TICKET_MAX_AGE_DAYS, VALIDATOR_QUORUM_SIZE};
use crate::ValidatorTicket;
use chrono::{Duration, Utc};
use itertools::Itertools;
use libp2p::PeerId;
use rand::seq::IteratorRandom;
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{mpsc, watch};
use tokio::{select, time};
use tracing::{error, info};

/// Seconds between raffle drawings
const RAFFLE_PERIOD_SECS: i64 = 10;

/// Path to the consensus database, from within the consensus data directory
const DATABASE_DIR: &str = "validators_db/";

/// Starts a raffle task, which periodically selects a new random set of validators from the set of
/// raffle tickets which have been entered.
pub fn start(
    config: &Config,
) -> (
    UnboundedSender<ValidatorTicket>,
    watch::Receiver<ValidatorQuorum>,
) {
    // Spawn the task
    let (ticket_sender, ticket_receiver) = mpsc::unbounded_channel();
    let (quorum_sender, quorum_receiver) = watch::channel(ValidatorQuorum(Vec::new()));
    tokio::spawn(task_fn(
        config.data_dir.join(DATABASE_DIR),
        ticket_receiver,
        quorum_sender,
    ));

    // Return the communication channels
    (ticket_sender, quorum_receiver)
}

/// The task function which runs the raffle
async fn task_fn(
    db_path: PathBuf,
    mut tickets_in: UnboundedReceiver<ValidatorTicket>,
    quorum_ch: watch::Sender<ValidatorQuorum>,
) {
    // Open the validator database
    let database =
        ValidatorDatabase::open(&db_path, true).expect("Failed to open validator database");

    // Start timer to start next raffle and draw new validators
    let mut raffle_timer = time::interval(Duration::seconds(RAFFLE_PERIOD_SECS).to_std().unwrap());

    // Event loop
    loop {
        select! {
            // Add new tickets to the raffle
            item = tickets_in.recv() => {
                match item {
                    None => {
                        error!("Ticket channel closed. Stopping raffle.");
                        return;
                    },
                    Some(ticket) => {
                        info!("Received ticket {} from peer {}", ticket.hash(), ticket.peer);
                        let mut wtxn = database.env.write_txn().unwrap();
                        database.db.put(&mut wtxn, &ValidatorDbKey::from(&ticket), &ticket)
                            .expect("Error writing ticket to database");
                        wtxn.commit().unwrap();
                    }
                }
            }

            // Periodically draw a new quorum from the raffle
            _ = raffle_timer.tick() => {
                // First, remove expired entries
                let now = Utc::now();
                let mut wtxn = database.env.write_txn().unwrap();
                let expired: Vec<_> = database.db.iter(&mut wtxn).unwrap().filter_map(|entry| {
                    if let Ok((key, ticket)) = entry {
                        if  now.signed_duration_since(ticket.time).num_days() > RAFFLE_TICKET_MAX_AGE_DAYS {
                            Some(key)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }).collect();
                for entry in expired {
                    database.db.delete(&mut wtxn, &entry).unwrap();
                }
                wtxn.commit().unwrap();

                // Next build a random quorum
                let rtxn = database.env.read_txn().unwrap();
                let iter_peers = database.db.iter(&rtxn).unwrap().filter_map(|t| {
                        if let Ok((_key, ticket)) = t {
                            return Some(ticket.peer);
                        } else {
                            None
                        }
                    }).unique();
                let quorum = if database.db.len(&rtxn).unwrap() > VALIDATOR_QUORUM_SIZE.try_into().unwrap() {
                    // If we have more than enough validators to fill the quorum, take a random sample.
                     iter_peers.choose_multiple(&mut thread_rng(), VALIDATOR_QUORUM_SIZE)
                } else {
                    // Otherwise, just collect as many unique validators as we have
                    iter_peers.collect()
                };
                rtxn.commit().unwrap();

                // Finally send the quorum
                quorum_ch.send(ValidatorQuorum::from(quorum)).unwrap();
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorQuorum(Vec<PeerId>);

impl From<Vec<PeerId>> for ValidatorQuorum {
    fn from(vec: Vec<PeerId>) -> Self {
        ValidatorQuorum(vec)
    }
}
