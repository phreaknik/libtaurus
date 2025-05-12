use libtaurus::{
    consensus::{self, api::ConsensusApi, dag},
    p2p::{self, P2pApi},
};
use tokio::select;
use tracing::error;

/// Event handler for all running processes
pub struct Handler {
    p2p_api: P2pApi,
    consensus_api: ConsensusApi,
}

impl Handler {
    /// Create a new handler
    pub fn new(p2p_api: P2pApi, consensus_api: ConsensusApi) -> Handler {
        Handler {
            p2p_api,
            consensus_api,
        }
    }

    /// Start the handler process
    pub fn start(self) {
        tokio::spawn(self.task_fn());
    }

    async fn task_fn(mut self) {
        let mut consensus_events = self.consensus_api.subscribe_events();
        let mut p2p_events = self.p2p_api.subscribe_events();
        loop {
            select! {
                // Handle P2P events
                event = p2p_events.recv() => {
                    match event {
                        Ok(p2p::Event::GossipsubMessage(message)) => {
                            let msg_validity = self.handle_gossipsub(message).await;
                            match self.p2p_api.report_message_validity(msg_validity) {
                                Err(p2p::api::Error::ActionSend(e)) => error!("Error sending P2P action: {e}"),
                                _ => {},
                            }
                        },
                        Ok(p2p::Event::Stopped) => todo!(),
                        Err(e) => return error!("Stopping due to p2p_events channel error: {e}"),
                    }
                },
                _event = consensus_events.recv() => {},
            }
        }
    }

    /// Handle a message from the GossipSub router
    async fn handle_gossipsub(&mut self, bcast: p2p::Broadcast) -> p2p::BroadcastValidationReport {
        match &bcast.data {
            p2p::BroadcastData::Vertex(vx) => match self.consensus_api.submit_vertex(&vx).await {
                Ok(_) => bcast.accept(),

                // Handle missing parents
                Err(consensus::api::Error::Task(consensus::task::Error::DAG(
                    dag::Error::MissingParents(missing),
                ))) => {
                    // Start a new fetcher for the missing parents
                    let p2p_api = self.p2p_api.clone();
                    let consensus_api = self.consensus_api.clone();
                    let initial = missing.clone();
                    tokio::spawn(async move {
                        if let Ok(mut fetcher) = p2p_api.get_fetcher().await {
                            fetcher.run(initial, consensus_api).await;
                        }
                    });
                    bcast.accept()
                }

                // Punative errors
                Err(consensus::api::Error::Task(consensus::task::Error::DAG(
                    dag::Error::BadHeight(_, _)
                    | dag::Error::ConflictingAncestors
                    | dag::Error::SelfReferentialParent
                    | dag::Error::Vertex(_),
                ))) => bcast.reject(),

                // Non-punative errors
                _ => bcast.ignore(),
            },
        }
    }
}
