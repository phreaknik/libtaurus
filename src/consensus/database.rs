use super::{block, vertex, BlockHash, Vertex};
use crate::{
    hash::{self, Hash},
    wire::{self, proto, WireFormat},
    VertexHash,
};
use heed::{BytesDecode, BytesEncode, Database, Env, EnvOpenOptions, RwTxn};
use serde_derive::{Deserialize, Serialize};
use std::{fs, path::PathBuf, result, sync::Arc};

/// Error type for consensus errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Block(#[from] block::Error),
    #[error("expected block")]
    ExpectedBlock,
    #[error("expected height link")]
    ExpectedHeightLink,
    #[error("expected vertex")]
    ExpectedVertex,
    #[error("expected vertex hash")]
    ExpectedVertexHash,
    #[error(transparent)]
    Hash(#[from] hash::Error),
    #[error(transparent)]
    Heed(#[from] heed::Error),
    #[error("protobuf message is missing data")]
    IncompleteResponse,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Vertex(#[from] vertex::Error),
    #[error(transparent)]
    Wire(#[from] wire::Error),
}

/// Result type for consensus errors
pub type Result<T> = result::Result<T, Error>;

/// Database to store consensus data, using the [`heed`] LMDB database wrapper.
#[derive(Clone)]
pub struct ConsensusDb {
    env: Env,
    db: Database<DbKey, DbRecord>,
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
    pub fn lookup_vertex_for_block<'a>(&'a self, bhash: &BlockHash) -> Result<Option<VertexHash>> {
        let mut rtxn = self.env.read_txn().unwrap();
        Ok(self
            .db
            .get(&mut rtxn, &self.link_key(bhash))?
            .map(|v| v.try_into().expect("corrupt database entry")))
    }

    /// Write a new vertex into the database
    /// May optionally pass in an existing write transaction, to add this to a batch of writes
    /// Must call [`heed::RwTxn::commit`] on the resulting [`RwTxn`] for the database write to
    /// complete.
    pub fn write_vertex<'a>(
        &'a mut self,
        wtxn: Option<RwTxn<'a, 'a>>,
        mined_height: u64,
        vertex: Arc<Vertex>,
    ) -> Result<RwTxn<'a, 'a>> {
        // Make sure this vertex includes the block. May not write slim vertices.
        if vertex.block.is_none() {
            return Err(Error::ExpectedBlock);
        };
        let vhash = vertex.hash();
        // Unwrap or create a new write transaction
        let mut wtxn = wtxn.unwrap_or(self.env.write_txn().unwrap());
        // Write the link from block-hash to vertex
        self.db.put(
            &mut wtxn,
            &self.link_key(&vertex.bhash),
            &DbRecord::from(vhash),
        )?;
        // Write the link from height to vertex
        let vertices_at_height = match self.db.get(&mut wtxn, &self.height_key(mined_height))? {
            Some(DbRecord::HeightLink(mut vertices)) => {
                vertices.push(vhash);
                Ok(DbRecord::HeightLink(vertices))
            }
            None => Ok(DbRecord::HeightLink(vec![vhash])),
            _ => Err(Error::ExpectedHeightLink),
        }?;
        self.db.put(
            &mut wtxn,
            &self.height_key(mined_height),
            &vertices_at_height,
        )?;
        // Write the vertex itsself
        self.db.put(
            &mut wtxn,
            &self.vertex_key(&vhash),
            &DbRecord::Vertex((mined_height, vertex)),
        )?;
        Ok(wtxn)
    }

    /// Read a vertex from the database
    pub fn read_vertex<'a>(&'a self, vhash: &VertexHash) -> Result<Option<Arc<Vertex>>> {
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

    /// Construct the db key for the specified blockhash-link object
    fn link_key(&self, bhash: &BlockHash) -> DbKey {
        DbKey(format!("link:{}", bhash.to_hex()))
    }

    /// Construct the db key for the specified height-link object
    fn height_key(&self, height: u64) -> DbKey {
        DbKey(format!("height:{height}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbKey(String);

impl<'a> BytesEncode<'a> for DbKey {
    type EItem = DbKey;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(item.0.as_bytes().into())
    }
}

impl<'a> BytesDecode<'a> for DbKey {
    type DItem = DbKey;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(DbKey(String::from_utf8(bytes.to_vec())?))
    }
}

#[derive(Debug, Clone)]
enum DbRecord {
    /// 'decided' DAG vertex to be stored
    Vertex((u64, Arc<Vertex>)),

    /// Link from [`BlockHash`] to the vertex it was decided in
    BlockLink(VertexHash),

    /// Link to the vertices mined at a given height
    HeightLink(Vec<VertexHash>),
}

impl<'a> WireFormat<'a, proto::DbRecord> for DbRecord {
    type Error = Error;

    fn to_protobuf(&self, check: bool) -> Result<proto::DbRecord> {
        match self {
            DbRecord::Vertex(v) => Ok(proto::DbRecord {
                RequestData: proto::mod_DbRecord::OneOfRequestData::vertex(proto::VertexRec {
                    height: v.0,
                    vertex: Some(v.1.to_protobuf(check)?),
                }),
            }),
            DbRecord::BlockLink(l) => Ok(proto::DbRecord {
                RequestData: proto::mod_DbRecord::OneOfRequestData::block_link(
                    l.to_protobuf(check)?,
                ),
            }),
            DbRecord::HeightLink(v) => Ok(proto::DbRecord {
                RequestData: proto::mod_DbRecord::OneOfRequestData::height_link(proto::Hashes {
                    hashes: v.into_iter().map(|v| v.to_protobuf(check)).try_collect()?,
                }),
            }),
        }
    }

    fn from_protobuf(record: &proto::DbRecord, check: bool) -> Result<DbRecord> {
        match &record.RequestData {
            proto::mod_DbRecord::OneOfRequestData::vertex(v) => Ok(DbRecord::Vertex((
                v.height,
                Arc::new(Vertex::from_protobuf(
                    v.vertex.as_ref().ok_or(Error::ExpectedVertex)?,
                    check,
                )?),
            ))),
            proto::mod_DbRecord::OneOfRequestData::block_link(l) => {
                Ok(DbRecord::BlockLink(Hash::from_protobuf(l, check)?))
            }
            proto::mod_DbRecord::OneOfRequestData::height_link(l) => Ok(DbRecord::HeightLink(
                l.hashes
                    .iter()
                    .map(|h| VertexHash::from_protobuf(h, check))
                    .try_collect()?,
            )),
            proto::mod_DbRecord::OneOfRequestData::None => Err(Error::IncompleteResponse),
        }
    }
}

impl TryInto<Arc<Vertex>> for DbRecord {
    type Error = Error;
    fn try_into(self) -> result::Result<Arc<Vertex>, Self::Error> {
        match self {
            DbRecord::Vertex(v) => {
                if v.1.block.is_none() {
                    Err(Error::ExpectedBlock)
                } else {
                    Ok(v.1)
                }
            }
            _ => Err(Error::ExpectedVertex),
        }
    }
}

impl From<VertexHash> for DbRecord {
    fn from(value: VertexHash) -> Self {
        DbRecord::BlockLink(value)
    }
}

impl TryInto<VertexHash> for DbRecord {
    type Error = Error;
    fn try_into(self) -> result::Result<VertexHash, Self::Error> {
        match self {
            DbRecord::BlockLink(l) => Ok(l),
            _ => Err(Error::ExpectedVertexHash),
        }
    }
}

impl<'a> BytesEncode<'a> for DbRecord {
    type EItem = DbRecord;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(item.to_wire(false)?.into())
    }
}

impl<'a> BytesDecode<'a> for DbRecord {
    type DItem = DbRecord;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(DbRecord::from_wire(bytes, false)?)
    }
}
