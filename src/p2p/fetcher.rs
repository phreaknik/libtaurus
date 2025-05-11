use crate::{
    consensus::{self, api::ConsensusApi},
    dag, Vertex, VertexHash,
};
use std::{collections::HashSet, sync::Arc};
use tokio::{select, sync::mpsc};

#[derive(Debug)]
pub struct Fetcher {
    request_ch: mpsc::UnboundedSender<VertexHash>,
    response_ch: mpsc::UnboundedReceiver<Arc<Vertex>>,
}

impl Fetcher {
    /// Create a new instance
    pub fn new() -> (
        Fetcher,
        mpsc::UnboundedReceiver<VertexHash>,
        mpsc::UnboundedSender<Arc<Vertex>>,
    ) {
        let (hash_sender, hash_receiver) = mpsc::unbounded_channel();
        let (vertex_sender, vertex_receiver) = mpsc::unbounded_channel();
        (
            Fetcher {
                request_ch: hash_sender,
                response_ch: vertex_receiver,
            },
            hash_receiver,
            vertex_sender,
        )
    }

    /// Run the fetcher process
    pub async fn run(&mut self, initial: Vec<VertexHash>, consensus_api: ConsensusApi) {
        // Keep track of which vertices we have already requested
        let mut requested = HashSet::new();

        // Helper to request a batch of hashes
        let mut request_hashes = |hashes: Vec<VertexHash>| {
            for &hash in &hashes {
                if !requested.contains(&hash) {
                    self.request_ch
                        .send(hash)
                        .expect("failed to request every missing hash");
                    requested.insert(hash);
                }
            }
        };

        // Ask the fetcher for the initial hashes
        request_hashes(initial);

        // Handle responses and request missing ancestors
        loop {
            select! {
                resp = self.response_ch.recv() => match resp {
                    // no more responses will come
                    None => return,

                    // Insert each found vertex
                    Some(vertex) => {
                        match consensus_api
                            .submit_vertex(&vertex)
                            .await {
                            Ok(_) => {},
                            Err(consensus::api::Error::Consensus(consensus::Error::DAG(dag::Error::MissingParents(parents))))
                            => request_hashes(parents),
                            Err(_) => {},
                        }
                    },
                }
            }
        }
    }
}
