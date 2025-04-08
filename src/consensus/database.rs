use super::{Block, SerdeHash};
use blake3::Hash;
use heed::{BytesDecode, BytesEncode, Database, Env, EnvOpenOptions, RwTxn};
use itertools::Itertools;
use libp2p::PeerId;
use rand::{seq::IteratorRandom, thread_rng};
use serde_derive::{Deserialize, Serialize};
use std::{fs, path::PathBuf, result};

/// Error type for consensus errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Block(#[from] super::block::Error),
    #[error(transparent)]
    Heed(#[from] heed::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    MsgPackDecode(#[from] rmp_serde::decode::Error),
    #[error(transparent)]
    MsgPackEncode(#[from] rmp_serde::encode::Error),
}

/// Result type for consensus errors
pub type Result<T> = result::Result<T, Error>;

/// Database to store consensus data, using the ['heed'] LMDB database wrapper.
#[derive(Clone)]
pub struct BlocksDatabase {
    pub env: Env,
    pub db: Database<SerdeHash, BlockEntry>,
}

impl BlocksDatabase {
    /// Open the database at the given path, or optionally create it if nonexistent
    pub fn open(path: &PathBuf, create: bool) -> Result<BlocksDatabase> {
        if create {
            fs::create_dir_all(path.as_path())?;
        }
        let env = EnvOpenOptions::new().open(path)?;
        let db = if create {
            env.create_database(None)?
        } else {
            env.open_database(None)?.unwrap()
        };
        Ok(BlocksDatabase { env, db })
    }

    /// Write a new block into the database
    /// May optionally pass in an existing write transaction, to add this to a batch of writes
    /// Must call ['heed::RwTxn::commit'] on the resulting ['RwTxn'] for the database write to
    /// complete.
    pub fn write_block<'a>(
        &'a mut self,
        wtxn: Option<RwTxn<'a, 'a>>,
        block: Block,
        canonical: bool,
    ) -> Result<RwTxn<'a, 'a>> {
        let mut wtxn = wtxn.unwrap_or(self.env.write_txn().unwrap());
        self.db.put(
            &mut wtxn,
            &block.hash()?.into(),
            &BlockEntry { block, canonical },
        )?;
        Ok(wtxn)
    }

    /// Read a block from the database
    pub fn read_block<'a>(&'a mut self, hash: Hash) -> Result<Option<Block>> {
        // TODO: this hash->copy->serdehash nonsense has to stop. Maybe serdehash should go and
        // just pass byte arrays?
        let mut rtxn = self.env.read_txn().unwrap();
        Ok(self
            .db
            .get(&mut rtxn, &hash.into())?
            .map(|entry| entry.block))
    }

    /// Select a random quorum of miners from the list of recently mined blocks. Will return a set
    /// of 'count' unique miners, where each miner's probability of selection is weighted
    /// proportionally to the number of blocks he has mined in the last 'age' blocks.
    pub fn select_random_miners(&mut self, count: usize, age: usize) -> Result<Vec<PeerId>> {
        let rtxn = self.env.read_txn().unwrap();
        let selected = self
            .db
            .iter(&rtxn)
            .unwrap()
            .take(age)
            .filter_map(|element| {
                if let Ok((_hash, block_entry)) = element {
                    return Some(block_entry.block.miner);
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BlockEntry {
    /// The actual block data
    block: Block,
    /// Is this block considered canonical?
    canonical: bool,
}

impl<'a> BytesEncode<'a> for BlockEntry {
    type EItem = BlockEntry;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(rmp_serde::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for BlockEntry {
    type DItem = BlockEntry;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(rmp_serde::from_slice(bytes)?)
    }
}
