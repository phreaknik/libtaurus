use super::database::ValidatorDatabase;
use crate::consensus::validators::database::ValidatorDbKey;
use crate::consensus::validators::DATABASE_DIR;
use crate::consensus::Config;
use crate::ValidatorTicket;
use chrono::Duration;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{mpsc, watch};
use tokio::{select, time};
use tracing::{debug, error};

/// Seconds between raffle drawings
const RAFFLE_PERIOD_SECS: i64 = 10;

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
    let mut database =
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
                        debug!("Received ticket {} from peer {}", ticket.hash(), ticket.peer);
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
                database.delete_expired().expect("Failed to expire validator tickets!");

                // Next build a random quorum
                let quorum = database.select_quorum().expect("Failed to select a quorum of validators!");

                // Finally send the quorum
                quorum_ch.send(quorum).unwrap();
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
