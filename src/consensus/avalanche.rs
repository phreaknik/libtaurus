use super::{database::BlocksDatabase, Block, Error, Result};
use crate::params;
use blake3::Hash;
use chrono::Utc;
use libp2p::PeerId;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tracing::info;
use tracing_mutex::stdsync::TracingRwLock;

/// Path to the peer database, from within the peer data directory
pub const DATABASE_DIR: &str = "blocks_db/";

/// Configuration details for the consensus process.
#[derive(Debug, Clone)]
pub struct Config {
    /// Path to the consensus data directory
    pub data_dir: PathBuf,

    /// Genesis block to serve as root vertex in the DAG
    pub genesis: Block,
}

/// Implementation of Avalanche DAG
pub struct DAG {
    vertices: HashMap<Hash, Arc<TracingRwLock<Vertex>>>,
    database: BlocksDatabase,
    genesis_hash: Hash,
}

impl DAG {
    /// Create a new DAG
    pub fn new(config: Config) -> DAG {
        // Create DAG instance
        let mut dag = DAG {
            vertices: HashMap::new(),
            database: BlocksDatabase::open(&config.data_dir.join(DATABASE_DIR), true)
                .expect("Failed to open blocks database"),
            genesis_hash: config.genesis.hash().unwrap(),
        };

        // Insert the genesis block
        dag.try_insert(config.genesis)
            .expect("Failed to insert genesis block");

        // Return the dag
        dag
    }

    /// Try to insert a block as a new vertex in the DAG
    pub fn try_insert(&mut self, block: Block) -> Result<()> {
        let hash = block.hash()?;

        // Make sure this block has parents to attach to the DAG
        if block.parents.len() == 0 && hash != self.genesis_hash {
            // This block has no parents to attach to the DAG
            Err(Error::MissingData)
        } else {
            // Write it to the database
            let wtxn = self.database.write_block(None, block.clone(), false)?;

            // Create a new vertex from this block, if we have the requisite parents
            let vertex = Arc::new(TracingRwLock::new(Vertex {
                parents: block
                    .parents
                    .iter()
                    .map(|hash| {
                        self.vertices
                            .get(&hash.into())
                            .ok_or(Error::MissingParent(hash.into()))
                            .map(|val| val.clone())
                    })
                    .try_collect()?,
                children: Vec::new(),
                block,
            }));

            // Commit the block to the database only now that it has passed all validation
            wtxn.commit()?;

            // Update each parent
            for parent in vertex
                .write()
                .map_err(|_| Error::WriteLock)?
                .parents
                .iter_mut()
            {
                parent
                    .write()
                    .map_err(|_| Error::WriteLock)?
                    .children
                    .push(vertex.clone());
            }

            // Add it to the collection of vertices
            self.vertices.insert(
                vertex.read().map_err(|_| Error::ReadLock)?.hash()?,
                vertex.clone(),
            );

            info!("Inserted block: {hash}");

            Ok(())
        }
    }
}

/// A vertex in the avalanche DAG. A vertex is essentially a block with parent & child links to
/// assist DAG operations.
#[derive(Debug, Clone)]
struct Vertex {
    /// Block represented by this vertex
    block: Block,

    /// Every vertex after the genesis vertex will have parents.
    parents: Vec<Arc<TracingRwLock<Vertex>>>,

    /// If this vertex is accepted into the DAG, it will be built upon and acquire children. This
    /// map will be empty when the vertex is first mined, and omitted from the marshalled output
    /// when the vertex is marshalled for storage or transmission.
    children: Vec<Arc<TracingRwLock<Vertex>>>,
}

impl Vertex {
    /// Compute the hash of the vertex. Note, this is simply the hash of the block represented by
    /// this vertex.
    pub fn hash(&self) -> Result<Hash> {
        self.block.hash()
    }
}

impl Into<Block> for Vertex {
    fn into(self) -> Block {
        self.block
    }
}

/// The frontier describes the highest vertices in the DAG
#[derive(Debug, Clone)]
pub struct Frontier(pub Vec<Block>);

impl Frontier {
    /// Compute the difficulty of the next vertex to build on this frontier
    pub fn next_difficulty(&self) -> u64 {
        self.0.iter().map(|h| h.difficulty).min().unwrap()
    }

    /// Compute a candidate block to mine atop the given frontier
    pub fn to_candidate(&self, miner: PeerId) -> Block {
        Block {
            version: params::PROTOCOL_VERSION,
            parents: self.0.iter().map(|v| v.hash().unwrap().into()).collect(),
            height: self.height() + 1,
            difficulty: self.next_difficulty(),
            miner,
            time: Utc::now(),
            nonce: 0,
        }
    }

    pub fn height(&self) -> u64 {
        self.0[0].height
    }
}
