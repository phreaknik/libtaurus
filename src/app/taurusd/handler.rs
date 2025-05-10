use std::error::Error;

use libtaurus::{
    consensus::{self, api::ConsensusApi, dag},
    hash,
    p2p::{self, P2pApi},
    vertex,
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
            p2p::BroadcastData::Vertex(vx) => self
                .consensus_api
                .submit_vertex(&vx)
                .await
                .map(|_| bcast.accept())
                .or_else(
                    |err| match err.source().and_then(|s| s.downcast_ref::<hash::Error>()) {
                        // All hash errors are punative errors
                        Some(_) => Ok(bcast.reject()),
                        None => Err(err),
                    },
                )
                .or_else(
                    |err| match err.source().and_then(|s| s.downcast_ref::<vertex::Error>()) {
                        // All vertex errors are punative errors
                        Some(_) => Ok(bcast.reject()),
                        None => Err(err),
                    },
                )
                .or_else(
                    |err| match err.source().and_then(|s| s.downcast_ref::<dag::Error>()) {
                        // Punative errors
                        Some(
                            dag::Error::BadHeight(_, _)
                            | dag::Error::ConflictingAncestors
                            | dag::Error::NoParents
                            | dag::Error::SelfReferentialParent,
                        ) => Ok(bcast.reject()),

                        // Non-punative errors
                        Some(
                            dag::Error::AlreadyInserted
                            | dag::Error::AlreadyRecorded
                            | dag::Error::MissingParents(_)
                            | dag::Error::NotFound
                            | dag::Error::RejectedAncestor
                            | dag::Error::WaitingOnVertex
                            | dag::Error::WaitingOnParents(_),
                        ) => Ok(bcast.ignore()),

                        // Patterns that must have been satisfied in a previous or_else
                        Some(
                            dag::Error::Hash(_)
                            | dag::Error::ProstDecode(_)
                            | dag::Error::ProstEncode(_)
                            | dag::Error::Vertex(_),
                        ) => unreachable!(),

                        None => Err(err),
                    },
                )
                .or_else(|err| {
                    match err
                        .source()
                        .and_then(|s| s.downcast_ref::<consensus::Error>())
                    {
                        // Non-punative errors
                        Some(
                            consensus::Error::EventsOutCh(_) | consensus::Error::P2pActionCh(_),
                        ) => Ok(bcast.ignore()),

                        // Patterns that must have been satisfied in a previous or_else
                        Some(consensus::Error::DAG(_) | consensus::Error::Vertex(_)) => {
                            unreachable!()
                        }

                        None => Err(err),
                    }
                })
                .or_else(|err| {
                    match err
                        .source()
                        .and_then(|s| s.downcast_ref::<consensus::api::Error>())
                    {
                        // Non-punative errors
                        Some(
                            consensus::api::Error::ActionSend(_)
                            | consensus::api::Error::ResponseRecv(_)
                            | consensus::api::Error::TimerElapsed(_),
                        ) => Ok(bcast.ignore()),

                        // Patterns that must have been satisfied in a previous or_else
                        Some(consensus::api::Error::Consensus(_)) => {
                            unreachable!()
                        }

                        None => Err(err),
                    }
                })
                .expect("unhandled error type"),
        }
    }
}
