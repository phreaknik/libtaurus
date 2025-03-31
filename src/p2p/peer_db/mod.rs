mod peer_info;

pub use self::peer_info::PeerInfo;
use core::result;
use libp2p::{multihash, PeerId};
use rkv::backend::{SafeMode, SafeModeDatabase, SafeModeDatabaseFlags, SafeModeEnvironment};
use rkv::{Manager, Rkv, SingleStore, Value};
use serde_cbor::to_vec;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use thiserror;
use tracing::error;

/// Error type for peer database errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Cbor(#[from] serde_cbor::Error),
    #[error(transparent)]
    Multihash(#[from] multihash::Error),
    #[error(transparent)]
    ManagerReadLock(#[from] PoisonError<RwLockReadGuard<'static, Manager<SafeModeEnvironment>>>),
    #[error(transparent)]
    ManagerWriteLock(#[from] PoisonError<RwLockWriteGuard<'static, Manager<SafeModeEnvironment>>>),
    #[error(transparent)]
    RkvReadLock(#[from] PoisonError<RwLockReadGuard<'static, Rkv<SafeModeEnvironment>>>),
    #[error(transparent)]
    RkvWriteLock(#[from] PoisonError<RwLockWriteGuard<'static, Rkv<SafeModeEnvironment>>>),
    #[error(transparent)]
    RkvStore(#[from] rkv::StoreError),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("item has unexpected type")]
    UnexpectedType,
    #[error("entry not found")]
    EntryNotFound,
}

pub type Result<T> = result::Result<T, Error>;

/// Database wrapper to implement a data store for peer information
#[derive(Clone)]
pub struct PeerDB {
    /// RKV manager instance to manage access to the peer database
    pub manager: Arc<RwLock<Rkv<SafeModeEnvironment>>>,

    /// Datastore
    pub store: SingleStore<SafeModeDatabase>,
}

impl PeerDB {
    /// Open the peer database at the given path
    pub fn open(path: &PathBuf) -> Result<PeerDB> {
        let mut manager = Manager::singleton().write()?;
        let shared_manager = manager.get_or_create(path.as_path(), Rkv::new::<SafeMode>)?;
        let env = shared_manager.read().unwrap();
        let store = env.open_single(
            "peer_db",
            rkv::StoreOptions {
                create: false,
                flags: SafeModeDatabaseFlags::empty(),
            },
        )?;
        Ok(PeerDB {
            manager: Arc::clone(&shared_manager),
            store,
        })
    }

    /// Open the peer database at the given path, or create it if nonexistent
    pub fn create_or_open(path: &PathBuf) -> Result<PeerDB> {
        fs::create_dir_all(path.as_path())?;
        let mut manager = Manager::singleton().write()?;
        let shared_manager = manager.get_or_create(path.as_path(), Rkv::new::<SafeMode>)?;
        let env = shared_manager.read().unwrap();
        let store = env.open_single("peer_db", rkv::StoreOptions::create())?;
        Ok(PeerDB {
            manager: Arc::clone(&shared_manager),
            store,
        })
    }

    /// Writes a peer to the database, overwriting it if it already exists
    pub fn write_peer_info(&mut self, data: &PeerInfo) -> Result<()> {
        let env = self.manager.read().unwrap();
        let mut writer = env.write()?;
        self.store.put(
            &mut writer,
            data.peer_id.to_bytes(),
            &Value::Blob(&to_vec(&data)?),
        )?;
        writer.commit()?;
        Ok(())
    }

    /// Reads a peer from the database
    pub fn read_peer_info(&self, peer_id: &PeerId) -> Result<PeerInfo> {
        let env = self.manager.read().unwrap();
        let reader = env.read()?;
        match self.store.get(&reader, peer_id.to_bytes())? {
            Some(Value::Blob(raw)) => Ok(serde_cbor::from_slice(&raw)?),
            _ => Err(Error::EntryNotFound),
        }
    }

    // List ['PeerId']s stored in the database
    pub fn list_peers(&self) -> Result<Vec<PeerId>> {
        let env = self.manager.read().unwrap();
        let reader = env.read()?;
        let saved_peers = self
            .store
            .iter_start(&reader)?
            .filter_map(|entry| entry.map(|(k, _v)| PeerId::from_bytes(k)).ok())
            .try_collect()?;
        Ok(saved_peers)
    }

    /// Iterate over peer entries and print them to the console
    pub fn print_peers(&self, max: Option<usize>) -> Result<()> {
        let env = self.manager.read().unwrap();
        let reader = env.read()?;
        self.store
            .iter_start(&reader)?
            .take(max.unwrap_or(usize::MAX))
            .try_for_each(|entry| {
                let (_k, v) = entry?;
                let info: PeerInfo = match v {
                    Value::Blob(raw) => serde_cbor::from_slice(&raw).map_err(Error::from),
                    _ => Err(Error::UnexpectedType),
                }?;
                println!("{info}");
                Ok::<(), Error>(())
            })?;
        Ok(())
    }
}
