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

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests the randomx hasher on known inputs
    #[test]
    fn hashes() -> Result<(), Error> {
        let test_key = b"randomx-test-key";
        struct TestCase<'a> {
            input: &'a [u8],
            hash: [u8; 32],
        }
        let test_cases = [
            TestCase {
                input: b"Hello, World!",
                hash: [
                    114, 148, 92, 219, 24, 248, 222, 108, 191, 236, 255, 150, 96, 165, 239, 158,
                    220, 148, 94, 166, 183, 33, 109, 25, 53, 183, 18, 57, 58, 119, 64, 154,
                ],
            },
            TestCase {
                input: b"Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.",
                hash: [
                    99, 216, 39, 255, 11, 252, 72, 49, 49, 184, 148, 130, 39, 14, 69, 5, 132, 130,
                    46, 234, 75, 228, 172, 190, 199, 241, 74, 91, 28, 24, 126, 132,
                ],
            },
        ];

        // Create a VM with a unique key, and confirm the hashes match
        let vm = RandomXVMInstance::new(test_key, RandomXFlag::get_recommended_flags())?;
        for test in &test_cases {
            let hash = vm.calculate_hash(test.input)?;
            assert_eq!(hash, test.hash);
        }

        // Create a new VM with a different key from before, and confirm the hashes do NOT match.
        let vm = RandomXVMInstance::new(b"randomx-bad-key", RandomXFlag::get_recommended_flags())?;
        for test in &test_cases {
            let hash = vm.calculate_hash(test.input)?;
            assert_ne!(hash, test.hash);
        }
        Ok(())
    }
}
