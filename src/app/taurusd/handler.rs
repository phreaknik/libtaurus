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
                Ok(_)
                | Err(consensus::api::Error::Consensus(consensus::Error::DAG(
                    dag::Error::MissingParents(_),
                ))) => bcast.accept(),
                Err(consensus::api::Error::Consensus(consensus::Error::DAG(
                    dag::Error::AlreadyInserted | dag::Error::RejectedAncestor,
                ))) => bcast.ignore(),
                Err(_) => bcast.reject(),
            },
        }
    }
}
