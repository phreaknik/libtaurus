#![feature(iterator_try_collect)]
#![feature(result_flattening)]
#![feature(assert_matches)]

//! An implementation of the Cordelia distributed ledger protocol. Cordelia is a combination of
//! proven technologies, which when combined facilitate a scalable, fungible, and programmable
//! payment system.
//!
//! Protocol overview:
//!   * Proof-of-work provides objective sybil resistance for strong network security.
//!   * Avalanche Consensus provides quick consensus with optimal finalization times.
//!   * zkRollups to scale transaction throughput and provide fungibility.
//!
//! Essentially, the protocol behaves as follows:
//! * miners build blocks with proof-of-work. Unlike other networks, a block does not commit to its
//!   parent(s). That job belongs to the vertex.
//! * miners do not get rewarded directly. Reward requires Proof-of-work AND participation in
//!   Avalanche consensus. Thus, miners also run a node to participate in consensus.
//! * nodes select blocks for inclusion in the dag. They build a vertex which links a block to some
//!   parents in the DAG, and share that vertex with peers.
//! * nodes are selected randomly to participate in avalanche queries. Only nodes who recently mined
//!   a block with sufficient proof-of-work are selected.
//! * once a node has served avalanche requests for many peers, it may submit a payout transaction
//!   for every block its mined.
//! * nodes keep a record of peers who are participating in Avalanche consensus. Nodes will only
//!   vote favorably for a payout transaction, if they deem the payee to have been an active
//!   avalanche participant AND have submitted appropriate proofs-of-work.
//!
//! * TODO: tokenomics: block rewards are "options", which may be exercised to claim rewards
//!   proportional to network hashrate at time of exercise.

pub mod consensus;
pub mod hash;
pub mod http;
pub mod miner;
pub mod p2p;
pub mod params;
pub mod randomx;
pub mod util;
pub mod wire;

// TODO: rename avalanche
pub use consensus::{avalanche, Block, BlockHash, GenesisConfig, Txo, TxoHash, Vertex, VertexHash};
pub use wire::WireFormat;
