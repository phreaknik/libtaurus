use crate::consensus::Result;
use heed::{BytesDecode, BytesEncode, Database, Env, EnvOpenOptions};
use serde_derive::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

/// Database of peer info, using the ['heed'] LMDB database wrapper.
#[derive(Clone)]
pub struct ValidatorDatabase {
    pub env: Env,
    pub db: Database<ValidatorDbKey, ValidatorDbEntry>,
}

impl ValidatorDatabase {
    /// Open the database at the given path, or optionally create it if nonexistent
    pub fn open(path: &PathBuf, create: bool) -> Result<ValidatorDatabase> {
        if create {
            fs::create_dir_all(path.as_path())?;
        }
        let env = EnvOpenOptions::new().open(path)?;
        let db = if create {
            env.create_database(None)?
        } else {
            env.open_database(None)?.unwrap()
        };
        Ok(ValidatorDatabase { env, db })
    }
}

/// Key by which consensus data is stored in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorDbKey {}

impl<'a> BytesEncode<'a> for ValidatorDbKey {
    type EItem = ValidatorDbKey;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(serde_cbor::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for ValidatorDbKey {
    type DItem = ValidatorDbKey;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(serde_cbor::from_slice(bytes)?)
    }
}

/// Consensus data types which may be stored in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidatorDbEntry {}

impl<'a> BytesEncode<'a> for ValidatorDbEntry {
    type EItem = ValidatorDbEntry;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(serde_cbor::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for ValidatorDbEntry {
    type DItem = ValidatorDbEntry;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(serde_cbor::from_slice(bytes)?)
    }
}
