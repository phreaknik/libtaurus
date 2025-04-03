use super::Result;
use heed::{BytesDecode, BytesEncode, Database, Env, EnvOpenOptions};
use serde_derive::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

/// Database of peer info, using the ['heed'] LMDB database wrapper.
#[derive(Clone)]
pub struct ConsensusDatabase {
    pub env: Env,
    pub db: Database<ConsensusDbKey, ConsensusData>,
}

impl ConsensusDatabase {
    /// Open the database at the given path, or optionally create it if nonexistent
    pub fn open(path: &PathBuf, create: bool) -> Result<ConsensusDatabase> {
        if create {
            fs::create_dir_all(path.as_path())?;
        }
        let env = EnvOpenOptions::new().open(path)?;
        let db = if create {
            env.create_database(None)?
        } else {
            env.open_database(None)?.unwrap()
        };
        Ok(ConsensusDatabase { env, db })
    }
}

/// Key by which consensus data is stored in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusDbKey {}

impl<'a> BytesEncode<'a> for ConsensusDbKey {
    type EItem = ConsensusDbKey;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(serde_cbor::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for ConsensusDbKey {
    type DItem = ConsensusDbKey;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(serde_cbor::from_slice(bytes)?)
    }
}

/// Consensus data types which may be stored in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusData {}

impl<'a> BytesEncode<'a> for ConsensusData {
    type EItem = ConsensusData;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(serde_cbor::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for ConsensusData {
    type DItem = ConsensusData;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(serde_cbor::from_slice(bytes)?)
    }
}
