use super::{block, vertex, BlockHash};
use crate::{VertexHash, WireVertex};
use heed::{BytesDecode, BytesEncode, Database, Env, EnvOpenOptions, RwTxn};
use serde_derive::{Deserialize, Serialize};
use std::{fs, path::PathBuf, result};

/// Error type for consensus errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Block(#[from] block::Error),
    #[error("expected block")]
    ExpectedBlock,
    #[error("expected vertex")]
    ExpectedVertex,
    #[error("expected vertex hash")]
    ExpectedVertexHash,
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
    env: Env,
    db: Database<DbKey, DbEntry>,
}

impl ConsensusDb {
    /// Open the database at the given path, or optionally create it if nonexistent
    pub fn open(path: &PathBuf, create: bool) -> Result<ConsensusDb> {
        if create {
            fs::create_dir_all(path.as_path())?;
        }
        let env = EnvOpenOptions::new().open(path)?;
        let db = if create {
            env.create_database(None)?
        } else {
            env.open_database(None)?.unwrap()
        };
        Ok(ConsensusDb { env, db })
    }

    /// Read a block from the database
    pub fn lookup_vertex_for_block<'a>(
        &'a mut self,
        bhash: &BlockHash,
    ) -> Result<Option<VertexHash>> {
        let mut rtxn = self.env.read_txn().unwrap();
        Ok(self
            .db
            .get(&mut rtxn, &self.link_key(bhash))?
            .map(|v| v.try_into().expect("corrupt database entry")))
    }

    /// Write a new vertex into the database
    /// May optionally pass in an existing write transaction, to add this to a batch of writes
    /// Must call ['heed::RwTxn::commit'] on the resulting ['RwTxn'] for the database write to
    /// complete.
    pub fn write_vertex<'a>(
        &'a mut self,
        wtxn: Option<RwTxn<'a, 'a>>,
        vertex: WireVertex,
    ) -> Result<RwTxn<'a, 'a>> {
        // Make sure this vertex includes the block. May not write slim vertices.
        if vertex.block.is_none() {
            return Err(Error::ExpectedBlock);
        };
        // Write the link from block-hash to vertex
        let mut wtxn = wtxn.unwrap_or(self.env.write_txn().unwrap());
        let vhash = vertex.hash();
        self.db.put(
            &mut wtxn,
            &self.link_key(&vertex.bhash),
            &DbEntry::from(vhash),
        )?;
        // Write the vertex itsself
        self.db.put(
            &mut wtxn,
            &self.vertex_key(&vhash),
            &DbEntry::from(vertex.slim().1),
        )?;
        Ok(wtxn)
    }

    /// Read a vertex from the database
    pub fn read_vertex<'a>(&'a mut self, vhash: &VertexHash) -> Result<Option<WireVertex>> {
        let mut rtxn = self.env.read_txn().unwrap();
        Ok(self
            .db
            .get(&mut rtxn, &self.vertex_key(vhash))?
            .map(|v| v.try_into().expect("corrupt database entry")))
    }

    /// Construct the db key for the specified vertex
    fn vertex_key(&self, vhash: &VertexHash) -> DbKey {
        DbKey(format!("vertex:{}", vhash.to_hex()))
    }

    /// Construct the db key for the specified link object
    fn link_key(&self, bhash: &BlockHash) -> DbKey {
        DbKey(format!("link:{}", bhash.to_hex()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbKey(String);

impl<'a> BytesEncode<'a> for DbKey {
    type EItem = DbKey;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(rmp_serde::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for DbKey {
    type DItem = DbKey;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(rmp_serde::from_slice(bytes)?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum DbEntry {
    Vertex(WireVertex),
    Link(VertexHash),
}

impl From<WireVertex> for DbEntry {
    fn from(value: WireVertex) -> Self {
        DbEntry::Vertex(value)
    }
}

impl TryInto<WireVertex> for DbEntry {
    type Error = Error;
    fn try_into(self) -> result::Result<WireVertex, Self::Error> {
        match self {
            DbEntry::Vertex(v) => {
                if v.block.is_none() {
                    Err(Error::ExpectedBlock)
                } else {
                    Ok(v)
                }
            }
            _ => Err(Error::ExpectedVertex),
        }
    }
}

impl From<VertexHash> for DbEntry {
    fn from(value: VertexHash) -> Self {
        DbEntry::Link(value)
    }
}

impl TryInto<VertexHash> for DbEntry {
    type Error = Error;
    fn try_into(self) -> result::Result<VertexHash, Self::Error> {
        match self {
            DbEntry::Link(l) => Ok(l),
            _ => Err(Error::ExpectedVertexHash),
        }
    }
}

impl<'a> BytesEncode<'a> for DbEntry {
    type EItem = DbEntry;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(rmp_serde::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for DbEntry {
    type DItem = DbEntry;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(rmp_serde::from_slice(bytes)?)
    }
}
