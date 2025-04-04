use super::database::ValidatorDatabase;
use crate::consensus::validators::database::ValidatorDbKey;
use crate::consensus::Config;
use crate::ValidatorTicket;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{mpsc, watch};
use tokio::{select, time};
use tracing::{error, info};

/// Period between raffle drawings
const RAFFLE_PERIOD_SECS: Duration = Duration::from_millis(10000);

/// Path to the consensus database, from within the consensus data directory
pub const DATABASE_DIR: &str = "validators_db/";

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
    let mut raffle_timer = time::interval(RAFFLE_PERIOD_SECS);

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

            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorQuorum(Vec<PeerId>);
