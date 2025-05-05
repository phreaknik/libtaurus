use std::result;
use tokio::{
    select,
    time::{interval, Duration},
};
use tracing::info;

#[derive(thiserror::Error, Debug)]
pub enum Error {}
type Result<T> = result::Result<T, Error>;

/// Configuration details for the sequencer process.
#[derive(Debug, Clone)]
pub struct Config {}

/// Run the consensus process, spawning the task as a new thread. Returns an [`broadcast::Sender`],
/// which can be subscribed to, to receive consensus events from the task.
pub fn start(config: Config) {
    // Spawn a task to execute the runtime
    let runtime = Runtime::new(config).expect("Failed to start sequencer runtime");
    tokio::spawn(runtime.run());
    loop {}
}

/// Runtime state for the consensus process
pub struct Runtime {
    _config: Config,
}

impl Runtime {
    fn new(config: Config) -> Result<Runtime> {
        info!("Starting sequencer...");

        // Instantiate the runtime
        Ok(Runtime {
            _config: config.clone(),
        })
    }

    // Run the consensus processing loop
    async fn run(self) {
        let mut timer = interval(Duration::from_secs(1));
        loop {
            select! {
                // Handle timer event
                _ = timer.tick() => {
                    info!("tick!");
                },
            }
        }
    }
}
