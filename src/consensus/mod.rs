use crate::p2p::{self, Message};
use std::time::{Duration, Instant};
use tokio::select;
use tokio::time::interval;
use tracing::{error, info};

/// Error type for cordelia-core errors
#[derive(thiserror::Error, Debug)]
pub enum Error {}

/// Configuration details for ['cordelia-core'].
#[derive(Debug, Clone)]
pub struct Config {}

/// Run the cordelia-core backend to handle consensus.
pub async fn run(_config: Config, mut p2p_api: p2p::Api) {
    info!("Starting consensus subsystem...");

    let mut ticker = interval(Duration::from_secs(5));
    let start = Instant::now();

    let mut p2p_events = p2p_api.subscribe();

    loop {
        select! {
            event = p2p_events.recv() => {
                    info!("received p2p event {event:?}");
            },
            _ = ticker.tick() => {
                match p2p_api.broadcast(Message::Hello(format!("I'm online for {:?}", start.elapsed()).into())) {
                    Ok(_) => info!("message sent!"),
                    Err(e) => error!("error sending message: {e}"),
                }
            },
        }
    }
}
