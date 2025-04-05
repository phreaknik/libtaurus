use randomx_rs::{RandomXCache, RandomXError, RandomXFlag, RandomXVM};
use std::sync::Arc;
use tracing_mutex::stdsync::TracingRwLock;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    RandomX(#[from] RandomXError),
    #[error("failed to grab randomx write-lock")]
    WriteLock,
}

/// Instance of the RandomX virtual machine
#[derive(Clone)]
pub struct RandomXVMInstance {
    instance: Arc<TracingRwLock<RandomXVM>>,
}

impl RandomXVMInstance {
    pub fn new(key: &[u8], flags: RandomXFlag) -> Result<Self, Error> {
        let cache = RandomXCache::new(flags, key).unwrap();
        let vm = RandomXVM::new(flags, Some(cache), None)?;

        // TODO: Today only light mode is enabled. Add option for fullmode, utilizing
        // RandomXFlag::FULL_MEM and RandomXFlag::LARGE_PAGES if possible

        Ok(Self {
            instance: Arc::new(TracingRwLock::new(vm)),
        })
    }

    /// Calculate the RandomX hash
    pub fn calculate_hash(&self, input: &[u8]) -> Result<Vec<u8>, Error> {
        let rx_instance = self.instance.write().map_err(|_| Error::WriteLock)?;
        rx_instance.calculate_hash(input).map_err(Error::from)
    }
}

// This type should be Send and Sync since it is wrapped in an Arc RwLock, but
// for some reason Rust and clippy don't see it automatically.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for RandomXVMInstance {}
unsafe impl Sync for RandomXVMInstance {}
