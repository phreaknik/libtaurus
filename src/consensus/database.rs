use super::{block, vertex, Block, BlockHash};
use crate::{SlimVertex, VertexHash};
use heed::{Database, Env, EnvOpenOptions, RwTxn};
use itertools::Itertools;
use libp2p::PeerId;
use rand::{seq::IteratorRandom, thread_rng};
use std::{fs, path::PathBuf, result};

/// Error type for consensus errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Block(#[from] block::Error),
    #[error(transparent)]
    Heed(#[from] heed::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    MsgPackDecode(#[from] rmp_serde::decode::Error),
    #[error(transparent)]
    MsgPackEncode(#[from] rmp_serde::encode::Error),
    #[error(transparent)]
    Vertex(#[from] vertex::Error),
}

/// Result type for consensus errors
pub type Result<T> = result::Result<T, Error>;

/// Database to store consensus data, using the ['heed'] LMDB database wrapper.
#[derive(Clone)]
pub struct ConsensusDb {
    pub block_env: Env,
    /// Database of blocks
    pub block_db: Database<BlockHash, Block>,
    pub vertex_env: Env,
    /// Database of vertices
    pub vertex_db: Database<VertexHash, SlimVertex>,
    pub links_env: Env,
    /// Database of links between blocks and vertices
    pub links_db: Database<BlockHash, VertexHash>,
}

impl ConsensusDb {
    /// Open the database at the given path, or optionally create it if nonexistent
    pub fn open(path: &PathBuf, create: bool) -> Result<ConsensusDb> {
        // The consensus database consists of two LMDB databases: a block database, and a vertex
        // database. We need to create or open both
        // TODO: Having distinct DBs makes it possible to have non-atomic writes. i.e. a block gets
        // written, but not its vertex. Or vice versa. Must use one DB and batch writes.
        let (block_env, block_db) = {
            let path = path.join("block_db/");
            if create {
                fs::create_dir_all(path.as_path())?;
            }
            let env = EnvOpenOptions::new().open(path)?;
            if create {
                (env.clone(), env.create_database(None)?)
            } else {
                (env.clone(), env.open_database(None)?.unwrap())
            }
        };
        let (vertex_env, vertex_db) = {
            let path = path.join("vertex_db/");
            if create {
                fs::create_dir_all(path.as_path())?;
            }
            let env = EnvOpenOptions::new().open(path)?;
            if create {
                (env.clone(), env.create_database(None)?)
            } else {
                (env.clone(), env.open_database(None)?.unwrap())
            }
        };
        let (links_env, links_db) = {
            let path = path.join("links_db/");
            if create {
                fs::create_dir_all(path.as_path())?;
            }
            let env = EnvOpenOptions::new().open(path)?;
            if create {
                (env.clone(), env.create_database(None)?)
            } else {
                (env.clone(), env.open_database(None)?.unwrap())
            }
        };
        Ok(ConsensusDb {
            block_env,
            block_db,
            vertex_env,
            vertex_db,
            links_env,
            links_db,
        })
    }

    /// Write a new block into the database
    /// May optionally pass in an existing write transaction, to add this to a batch of writes
    /// Must call ['heed::RwTxn::commit'] on the resulting ['RwTxn'] for the database write to
    /// complete.
    pub fn write_block<'a>(
        &'a mut self,
        wtxn: Option<RwTxn<'a, 'a>>,
        block: &Block,
    ) -> Result<RwTxn<'a, 'a>> {
        let mut wtxn = wtxn.unwrap_or(self.block_env.write_txn().unwrap());
        self.block_db.put(&mut wtxn, &block.hash()?, block)?;
        Ok(wtxn)
    }

    /// Read a block from the database
    pub fn read_block<'a>(&'a mut self, bhash: &BlockHash) -> Result<Option<Block>> {
        let mut rtxn = self.block_env.read_txn().unwrap();
        Ok(self.block_db.get(&mut rtxn, bhash)?)
    }

    /// Read a block from the database
    pub fn lookup_vertex_for_block<'a>(
        &'a mut self,
        bhash: &BlockHash,
    ) -> Result<Option<VertexHash>> {
        let mut rtxn = self.links_env.read_txn().unwrap();
        Ok(self.links_db.get(&mut rtxn, bhash)?)
    }

    /// Write a new vertex into the database
    /// May optionally pass in an existing write transaction, to add this to a batch of writes
    /// Must call ['heed::RwTxn::commit'] on the resulting ['RwTxn'] for the database write to
    /// complete.
    pub fn write_vertex<'a>(&'a mut self, vertex: &SlimVertex) -> Result<()> {
        let vhash = vertex.hash()?;
        let mut wtxn_l = self.links_env.write_txn().unwrap();
        let mut wtxn_v = self.vertex_env.write_txn().unwrap();
        self.vertex_db.put(&mut wtxn_v, &vertex.hash()?, vertex)?;
        self.links_db.put(&mut wtxn_l, &vertex.block_hash, &vhash)?;
        Ok(())
    }

    /// Read a vertex from the database
    pub fn read_vertex<'a>(&'a mut self, vhash: &VertexHash) -> Result<Option<SlimVertex>> {
        let mut rtxn = self.vertex_env.read_txn().unwrap();
        Ok(self.vertex_db.get(&mut rtxn, vhash)?)
    }

    /// Select a random quorum of miners from the list of recently mined blocks. Will return a set
    /// of 'count' unique miners, where each miner's probability of selection is weighted
    /// proportionally to the number of blocks he has mined in the last 'age' blocks.
    pub fn select_random_miners(&mut self, count: usize, age: usize) -> Result<Vec<PeerId>> {
        // TODO: miner selection needs to come from _all_ blocks, not just blocks finalized and
        // accepted into the database.
        let rtxn = self.block_env.read_txn().unwrap();
        let selected = self
            .block_db
            .iter(&rtxn)?
            .take(age) //TODO: this assumes db is sorted by age... is it?
            .filter_map(|element| {
                if let Ok((_hash, block)) = element {
                    return Some(block.miner);
                } else {
                    None
                }
            })
            .unique()
            .choose_multiple(&mut thread_rng(), count);
        rtxn.commit().unwrap();
        Ok(selected)
    }
}
