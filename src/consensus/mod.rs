use crate::p2p::message::Message;
use std::result;
use std::time::{Duration, Instant};
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::interval;
use tracing::{error, info};

/// Error type for cordelia-core errors
#[derive(thiserror::Error, Debug)]
pub enum Error {}

/// Result type for cordelia-core
pub type Result<T> = result::Result<T, Error>;

/// Configuration details for ['cordelia-core'].
#[derive(Debug, Clone)]
pub struct Config {}

/// Run the cordelia-core backend to handle consensus.
pub async fn run(
    _config: &Config,
    mut msg_in: UnboundedReceiver<Message>,
    msg_out: UnboundedSender<Message>,
) -> Result<()> {
    let mut ticker = interval(Duration::from_secs(5));
    let start = Instant::now();

    loop {
        select! {
            msg = msg_in.recv() => {
                if let Some(message) = msg {
                    info!("received message {message:?}");
                }
            },
            _ = ticker.tick() => {
                match msg_out.send(Message::Hello(format!("I'm online for {:?}", start.elapsed()).into())) {
                    Err(e) => {error!("Failed to publish message: {e}");},
                    Ok(_) => {info!("Published message!");},
                }
            },
        }
    }
}
