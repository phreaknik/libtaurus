use super::{block, vertex, BlockHash};
use crate::{
    hash::{self, Hash},
    p2p::consensus_rpc::proto,
    wire::{self, WireFormat},
    VertexHash,
};
use heed::{BytesDecode, BytesEncode, Database, Env, EnvOpenOptions, RwTxn};
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
use serde_derive::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf, result, sync::Arc};

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
    Hash(#[from] hash::Error),
    #[error(transparent)]
    Heed(#[from] heed::Error),
    #[error("protobuf message is missing data")]
    IncompleteResponse,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    // TODO: use protobuf instead of rmp_serde for encode/decode
    #[error(transparent)]
    MsgPackDecode(#[from] rmp_serde::decode::Error),
    #[error(transparent)]
    MsgPackEncode(#[from] rmp_serde::encode::Error),
    #[error(transparent)]
    Vertex(#[from] vertex::Error),
    #[error(transparent)]
    Wire(#[from] wire::Error),
}

/// Result type for consensus errors
pub type Result<T> = result::Result<T, Error>;

/// Database to store consensus data, using the ['heed'] LMDB database wrapper.
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
    /// Must call ['heed::RwTxn::commit'] on the resulting ['RwTxn'] for the database write to
    /// complete.
    pub fn write_vertex<'a>(
        &'a mut self,
        wtxn: Option<RwTxn<'a, 'a>>,
        vertex: Arc<wire::Vertex>,
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
            &DbRecord::from(vhash),
        )?;
        // Write the vertex itsself
        self.db
            .put(&mut wtxn, &self.vertex_key(&vhash), &DbRecord::from(vertex))?;
        Ok(wtxn)
    }

    /// Read a vertex from the database
    pub fn read_vertex<'a>(&'a self, vhash: &VertexHash) -> Result<Option<Arc<wire::Vertex>>> {
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

#[derive(Debug, Clone)]
enum DbRecord {
    Vertex(Arc<wire::Vertex>),
    Link(VertexHash),
}

impl DbRecord {
    /// Deserialize from protobuf format
    pub fn from_protobuf(record: &proto::DbRecord) -> Result<DbRecord> {
        match &record.RequestData {
            proto::mod_DbRecord::OneOfRequestData::vertex(v) => {
                Ok(DbRecord::Vertex(Arc::new(wire::Vertex::from_protobuf(v)?)))
            }
            proto::mod_DbRecord::OneOfRequestData::link(l) => {
                Ok(DbRecord::Link(Hash::from_protobuf(l)?))
            }
            proto::mod_DbRecord::OneOfRequestData::None => Err(Error::IncompleteResponse),
        }
    }

    /// Serialize into protobuf format
    pub fn to_protobuf(&self) -> Result<proto::DbRecord> {
        match self {
            DbRecord::Vertex(v) => Ok(proto::DbRecord {
                RequestData: proto::mod_DbRecord::OneOfRequestData::vertex(v.to_protobuf()?),
            }),
            DbRecord::Link(l) => Ok(proto::DbRecord {
                RequestData: proto::mod_DbRecord::OneOfRequestData::link(l.to_protobuf()?),
            }),
        }
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<DbRecord> {
        let protobuf = proto::DbRecord::from_reader(&mut BytesReader::from_bytes(bytes), &bytes)
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unable to parse block from bytes: {e}"),
                )
            })?;
        DbRecord::from_protobuf(&protobuf)
    }

    /// Serialize into bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        let protobuf = self.to_protobuf().map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to convert DbRecord to protobuf: {e}"),
            )
        })?;
        protobuf.write_message(&mut writer).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "unable to serialize DbRecord")
        })?;
        Ok(bytes)
    }
}

impl From<Arc<wire::Vertex>> for DbRecord {
    fn from(value: Arc<wire::Vertex>) -> Self {
        DbRecord::Vertex(value)
    }
}

impl TryInto<Arc<wire::Vertex>> for DbRecord {
    type Error = Error;
    fn try_into(self) -> result::Result<Arc<wire::Vertex>, Self::Error> {
        match self {
            DbRecord::Vertex(v) => {
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

impl From<VertexHash> for DbRecord {
    fn from(value: VertexHash) -> Self {
        DbRecord::Link(value)
    }
}

impl TryInto<VertexHash> for DbRecord {
    type Error = Error;
    fn try_into(self) -> result::Result<VertexHash, Self::Error> {
        match self {
            DbRecord::Link(l) => Ok(l),
            _ => Err(Error::ExpectedVertexHash),
        }
    }
}

impl<'a> BytesEncode<'a> for DbRecord {
    type EItem = DbRecord;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(item.to_bytes()?.into())
    }
}

impl<'a> BytesDecode<'a> for DbRecord {
    type DItem = DbRecord;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(DbRecord::from_bytes(bytes)?)
    }
}
