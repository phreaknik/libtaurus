use super::ConsensusApi;
use crate::{
    p2p::{self, P2pApi},
    VertexHash,
};
use libp2p::PeerId;
use std::collections::HashSet;
use tokio::{select, sync::mpsc};
use tracing::debug;

/// Start a new [`Pollster`] instance
pub fn start(
    vhash: VertexHash,
    p2p_api: P2pApi,
    consensus_api: ConsensusApi,
    peers_to_poll: HashSet<PeerId>,
    quorum_size: usize,
) {
    let task = Pollster::new(vhash, p2p_api, consensus_api, peers_to_poll, quorum_size);
    tokio::spawn(task.run());
}

pub(crate) struct Pollster {
    vhash: VertexHash,
    p2p_api: P2pApi,
    consensus_api: ConsensusApi,
    peers_to_poll: HashSet<PeerId>,
    quorum_size: usize,
}

impl Pollster {
    /// Create a new pollster instance to query peer preferences for the given vertex
    pub fn new(
        vhash: VertexHash,
        p2p_api: P2pApi,
        consensus_api: ConsensusApi,
        peers_to_poll: HashSet<PeerId>,
        quorum_size: usize,
    ) -> Pollster {
        Pollster {
            vhash,
            p2p_api,
            consensus_api,
            peers_to_poll,
            quorum_size,
        }
    }

    /// Query peers and collect their preferences for the specified vertex
    pub async fn run(self) {
        // Query each peer for their preference
        let (pref_sender, mut pref_receiver) = mpsc::unbounded_channel();
        for peer in &self.peers_to_poll {
            self.p2p_api
                .request(
                    p2p::Request::GetPreference(self.vhash),
                    pref_sender.clone(),
                    Some(*peer),
                )
                .expect("failed to send p2p action");
        }

        // TODO: make sure p2p punishes peers that respond invalidly or after timeout

        // Collect query results, exiting early once quorum has been reached
        let mut count_for = 0;
        let mut count_against = 0;
        loop {
            select! {
                Some(p2p::Response::Preference(pref)) = pref_receiver.recv() => {
                    if pref { count_for += 1} else { count_against += 1}
                    let done = count_against >= self.quorum_size || count_against >= self.quorum_size || count_against + count_for >= self.peers_to_poll.len();
                    if done {
                        let preferred = count_for >= self.quorum_size;
                        if preferred {
                            debug!("peers prefer {}", self.vhash.to_hex());
                        } else {
                            debug!("peers do not prefer {}", self.vhash.to_hex());
                        }
                        let _ = self.consensus_api.record_peer_preference(self.vhash, preferred);
                        return;
                    }
                },
            }
        }
    }
}
