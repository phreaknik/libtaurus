use crate::{
    consensus::{self, api::ConsensusApi},
    dag,
    p2p::{self, P2pApi, Response},
    VertexHash,
};
use libp2p::PeerId;
use std::collections::HashSet;
use tokio::{select, sync::mpsc};
use tracing::warn;

/// Start a fetcher task in its own thread
pub fn start(peer: PeerId, initial: Vec<VertexHash>, p2p_api: P2pApi, consensus_api: ConsensusApi) {
    tokio::spawn(Task::new(peer, initial, p2p_api, consensus_api).task_fn());
}

/// State for a running fetch process
pub struct Task {
    /// Initial peer to source data from
    peer: PeerId,

    /// Initial vertices to fetch
    initial: Vec<VertexHash>,

    /// List of every vertex we've requested
    requested: HashSet<VertexHash>,

    /// P2P API to request vertices
    p2p_api: P2pApi,

    /// Consensus API to submit vertices
    consensus_api: ConsensusApi,
}

impl Task {
    /// Create a new instance
    pub fn new(
        peer: PeerId,
        initial: Vec<VertexHash>,
        p2p_api: P2pApi,
        consensus_api: ConsensusApi,
    ) -> Task {
        Task {
            peer,
            initial,
            requested: HashSet::new(),
            p2p_api,
            consensus_api,
        }
    }

    /// Run the fetcher process
    pub async fn task_fn(mut self) {
        // Setup channels
        let (resp_sender, mut resp_receiver) = mpsc::unbounded_channel();

        // Helper to request a batch of hashes
        let mut request_hashes = |hashes| {
            for &hash in &hashes {
                if !self.requested.contains(&hash) {
                    let _ = self.p2p_api.request(
                        p2p::Request::GetVertex(hash),
                        resp_sender.clone(),
                        Some(self.peer),
                    );
                    self.requested.insert(hash);
                }
            }
        };

        // Ask the fetcher for the initial hashes
        request_hashes(self.initial.clone());

        // Handle responses and request missing ancestors
        loop {
            select! {
                resp = resp_receiver.recv() => match resp {
                    // no more responses will come
                    None => return,

                    // Insert each found vertex
                    Some(Response::Vertex(vertex)) => {
                        match self.consensus_api
                            .submit_vertex(&vertex)
                            .await {
                            Ok(_) => {},
                            Err(consensus::api::Error::Task(consensus::task::Error::DAG(dag::Error::MissingParents(parents))))
                            => request_hashes(parents),
                            Err(_) => {},
                        }
                    },

                    // Handle error
                    Some(Response::Error(code)) => warn!("error fetching (code = {code})"),

                    // Other responses to our request are illegal
                    _ => {
                        let _ = self.p2p_api.block_peer(self.peer);
                        return;
                    }
                }
            }
        }
    }
}
