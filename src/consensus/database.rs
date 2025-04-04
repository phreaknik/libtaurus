use crate::{
    consensus::{hash::Hash, Result},
    Block,
};
use chrono::Utc;
use heed::{BytesDecode, BytesEncode, Database, Env, EnvOpenOptions};
use itertools::Itertools;
use libp2p::PeerId;
use rand::seq::IteratorRandom;
use rand::thread_rng;
use serde_derive::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

/// Database to store consensus data, using the ['heed'] LMDB database wrapper.
#[derive(Clone)]
pub struct BlocksDatabase {
    pub env: Env,
    pub db: Database<Hash, BlockEntry>,
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
    pub fn write_block(&mut self, block: Block, canonical: bool) -> Result<()> {
        let mut wtxn = self.env.write_txn().unwrap();
        self.db
            .put(&mut wtxn, &block.hash(), &BlockEntry { block, canonical })?;
        wtxn.commit()?;
        Ok(())
    }

    ///// Select a random quorum of miners from the list of recently mined blocks. Will return a set
    ///// of 'count' unique miners, where each miner's probability of selection is weighted
    ///// proportionally to the number of blocks he has mined in the last 'age' blocks.
    //pub fn select_random_miners(&mut self, count: usize, age: usize) -> Result<Vec<PeerId>> {
    //    let rtxn = self.env.read_txn().unwrap();
    //    let selected = self
    //        .db
    //        .iter(&rtxn)
    //        .unwrap()
    //        .take(age)
    //        .filter_map(|element| {
    //            if let Ok((hash, block_entry)) = element {
    //                return Some(block_entry.block.header.miner);
    //            } else {
    //                None
    //            }
    //        })
    //        .unique()
    //        .choose_multiple(&mut thread_rng(), count);
    //    rtxn.commit().unwrap();
    //    Ok(selected)
    //}
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
        Ok(serde_cbor::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for BlockEntry {
    type DItem = BlockEntry;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(serde_cbor::from_slice(bytes)?)
    }
}
