use crate::consensus::{hash::Hash, validators::ValidatorTicket, Result};
use crate::params::{RAFFLE_TICKET_MAX_AGE_DAYS, VALIDATOR_QUORUM_SIZE};
use chrono::Utc;
use heed::{BytesDecode, BytesEncode, Database, Env, EnvOpenOptions};
use itertools::Itertools;
use rand::seq::IteratorRandom;
use rand::thread_rng;
use serde_derive::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

use super::ValidatorQuorum;

/// Database of peer info, using the ['heed'] LMDB database wrapper.
#[derive(Clone)]
pub struct ValidatorDatabase {
    pub env: Env,
    pub db: Database<ValidatorDbKey, ValidatorTicket>,
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

    /// Deletes any expired validator tickets from the database. Returns the number of entries
    /// that were deleted.
    pub fn delete_expired(&mut self) -> Result<usize> {
        let now = Utc::now();
        let mut wtxn = self.env.write_txn().unwrap();
        let expired: Vec<_> = self
            .db
            .iter(&mut wtxn)
            .unwrap()
            .filter_map(|entry| {
                if let Ok((key, ticket)) = entry {
                    if now.signed_duration_since(ticket.time).num_days()
                        > RAFFLE_TICKET_MAX_AGE_DAYS
                    {
                        Some(key)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        for entry in &expired {
            self.db.delete(&mut wtxn, &entry).unwrap();
        }
        wtxn.commit().unwrap();
        Ok(expired.len())
    }

    /// Select a random quorum of validators from the validator tickets in the database. Each
    /// validator's probability of being selected is proportional to the number of tickets they
    /// have in the database.
    pub fn select_quorum(&mut self) -> Result<ValidatorQuorum> {
        let rtxn = self.env.read_txn().unwrap();
        let iter_peers = self
            .db
            .iter(&rtxn)
            .unwrap()
            .filter_map(|t| {
                if let Ok((_key, ticket)) = t {
                    return Some(ticket.peer);
                } else {
                    None
                }
            })
            .unique();
        let quorum = if self.db.len(&rtxn).unwrap() > VALIDATOR_QUORUM_SIZE.try_into().unwrap() {
            // If we have more than enough validators to fill the quorum, take a random sample.
            iter_peers.choose_multiple(&mut thread_rng(), VALIDATOR_QUORUM_SIZE)
        } else {
            // Otherwise, just collect as many unique validators as we have
            iter_peers.collect()
        };
        rtxn.commit().unwrap();
        Ok(ValidatorQuorum::from(quorum))
    }
}

/// Key by which consensus data is stored in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorDbKey(Hash);

impl From<&ValidatorTicket> for ValidatorDbKey {
    fn from(ticket: &ValidatorTicket) -> Self {
        ValidatorDbKey(ticket.hash())
    }
}

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
