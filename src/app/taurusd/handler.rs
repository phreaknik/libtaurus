use chrono::Utc;
use libtaurus::{
    consensus::{self, dag, ConsensusApi},
    fetcher,
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
                event_res = p2p_events.recv() => {
                    let t_start = Utc::now();
                    match event_res {
                        Ok(event) => {
                            match event {
                                p2p::Event::GossipsubMessage(message) => {
                                    let msg_validity = self.handle_gossipsub(message).await;
                                    match self.p2p_api.report_message_validity(msg_validity) {
                                        Err(p2p::api::Error::ActionSend(e)) => error!("Error sending P2P action: {e}"),
                                        _ => {},
                                    }
                                },
                                p2p::Event::NewPeer(peer) => {let _ = self.consensus_api.add_validator(peer);},
                                p2p::Event::RequestMessage{request_id, request} => {
                                    let response = match request {
                                        p2p::Request::GetVertex(vhash) => {
                                            match self.consensus_api.get_vertex(vhash).await {
                                                Ok(Some(vx)) => p2p::Response::Vertex(vx),
                                                Ok(None) => p2p::Response::Error(p2p::request::ErrorCode::NotFound),
                                                Err(_) => p2p::Response::Error(p2p::request::ErrorCode::Unknown),
                                            }
                                        },
                                        p2p::Request::GetPreference(vhash) => {
                                            match self.consensus_api.get_preference(vhash).await {
                                                Ok(Some(pref)) => p2p::Response::Preference(pref),
                                                Ok(None) => p2p::Response::Error(p2p::request::ErrorCode::NotFound),
                                                Err(_) => p2p::Response::Error(p2p::request::ErrorCode::Unknown),
                                            }
                                        },
                                    };
                                    let _ =self.p2p_api.respond(request_id, response);
                                },
                            }
                            let duration = Utc::now() - t_start;
                            println!(":::: handled p2p event in {:0>3}.{:0>3}ms",
                                duration.num_milliseconds(),
                                duration.num_microseconds().unwrap()
                            );
                        }
                        Err(e) => return error!("Stopping due to p2p_events channel error: {e}"),
                    }
                },
                event = consensus_events.recv() => {
                    match event {
                        Ok(consensus::task::Event::NewFrontier(_frontier)) => {},
                        Err(e) => return error!("Stopping due to consensus_events channel error: {e}"),
                    }
                },
            }
        }
    }

    /// Handle a message from the GossipSub router
    async fn handle_gossipsub(
        &mut self,
        bcast: p2p::broadcast::Broadcast,
    ) -> p2p::broadcast::BroadcastValidationReport {
        match &bcast.data {
            p2p::broadcast::BroadcastData::Vertex(vx) => {
                match self.consensus_api.submit_vertex(&vx).await {
                    Ok(_) => bcast.accept(),

                    // Handle missing parents
                    Err(consensus::api::Error::Task(consensus::task::Error::DAG(
                        dag::Error::MissingParents(missing),
                    ))) => {
                        // Start a fetcher to request missing ancestors until the graph connects
                        fetcher::start(
                            bcast.src,
                            missing,
                            self.p2p_api.clone(),
                            self.consensus_api.clone(),
                        );
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
                }
            }
        }
    }
}
