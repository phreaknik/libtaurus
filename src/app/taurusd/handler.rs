use chrono::Utc;
use libp2p::request_response::InboundRequestId;
use libtaurus::{
    consensus::{self, dag, ConsensusApi},
    fetcher,
    p2p::{self, P2pApi},
};
use tokio::select;
use tracing::error;

#[derive(Debug, Default, Clone)]
pub struct Config {}

/// Event handler for all running processes
#[derive(Debug, Clone)]
pub struct Handler {
    _config: Config,
    p2p_api: P2pApi,
    consensus_api: ConsensusApi,
}

impl Handler {
    /// Create a new handler
    pub fn new(config: Config, p2p_api: P2pApi, consensus_api: ConsensusApi) -> Handler {
        Handler {
            _config: config,
            p2p_api,
            consensus_api,
        }
    }

    /// Start the handler process
    pub fn start(self) {
        tokio::spawn(self.task_fn());
    }

    async fn task_fn(mut self) {
        // Catch events and dispatch them to the workers
        let mut consensus_events = self.consensus_api.subscribe_events();
        let mut p2p_events = self.p2p_api.subscribe_events();
        loop {
            select! {
                event_res = p2p_events.recv() => match event_res {
                    Ok(event)  => match event {
                        p2p::Event::GossipsubMessage(message) => self.handle_gossipsub(message).await,
                        p2p::Event::NewPeer(peer) => {
                            let _ = self.consensus_api.add_validator(peer);
                        }
                        p2p::Event::RequestMessage {
                            request_id,
                            request,
                        } => self.handle_request_response(request_id, request).await,
                    },
                    Err(e) => error!("p2p event error: {e}"),
                },
                event_res = consensus_events.recv() => match event_res {
                    Ok(event)  => match event {
                        consensus::task::Event::NewFrontier(_frontier) => {},
                    },
                    Err(e) => error!("consensus event error: {e}"),
                },
            }
        }
    }

    /// Handle a message from the RequestResponse protocol
    async fn handle_request_response(&mut self, rid: InboundRequestId, request: p2p::Request) {
        let t_start = Utc::now();
        let response = match request {
            p2p::Request::GetVertex(vhash) => match self.consensus_api.get_vertex(vhash).await {
                Ok(Some(vx)) => p2p::Response::Vertex(vx),
                Ok(None) => p2p::Response::Error(p2p::request::ErrorCode::NotFound),
                Err(_) => p2p::Response::Error(p2p::request::ErrorCode::Unknown),
            },
            p2p::Request::GetPreference(vhash) => {
                match self.consensus_api.get_preference(vhash).await {
                    Ok(Some(pref)) => p2p::Response::Preference(pref),
                    Ok(None) => p2p::Response::Error(p2p::request::ErrorCode::NotFound),
                    Err(_) => p2p::Response::Error(p2p::request::ErrorCode::Unknown),
                }
            }
        };
        let _ = self.p2p_api.respond(rid, response);
        let duration = Utc::now() - t_start;
        println!(
            ":::: handled request message in {:0>3}.{:0>3}ms",
            duration.num_milliseconds(),
            duration.num_microseconds().unwrap()
        );
    }

    /// Handle a message from the GossipSub protocol
    async fn handle_gossipsub(&mut self, msg: p2p::broadcast::Broadcast) {
        let t_start = Utc::now();
        let msg_validity = match &msg.data {
            p2p::broadcast::BroadcastData::Vertex(vx) => {
                match self.consensus_api.submit_vertex(&vx).await {
                    Ok(_) => msg.accept(),

                    // Handle missing parents
                    Err(consensus::api::Error::Task(consensus::task::Error::DAG(
                        dag::Error::MissingParents(missing),
                    ))) => {
                        // Start a fetcher to request missing ancestors until the graph
                        // connects
                        fetcher::start(
                            msg.src,
                            missing,
                            self.p2p_api.clone(),
                            self.consensus_api.clone(),
                        );
                        msg.accept()
                    }

                    // Punative errors
                    Err(consensus::api::Error::Task(consensus::task::Error::DAG(
                        dag::Error::BadHeight(_, _)
                        | dag::Error::ConflictingAncestors
                        | dag::Error::SelfReferentialParent
                        | dag::Error::Vertex(_),
                    ))) => msg.reject(),

                    // Non-punative errors
                    _ => msg.ignore(),
                }
            }
        };
        match self.p2p_api.report_message_validity(msg_validity) {
            Err(p2p::api::Error::ActionSend(e)) => error!("Error sending P2P action: {e}"),
            _ => {}
        }
        let duration = Utc::now() - t_start;
        println!(
            ":::: handled gossipsub message in {:0>3}.{:0>3}ms",
            duration.num_milliseconds(),
            duration.num_microseconds().unwrap()
        );
    }
}
