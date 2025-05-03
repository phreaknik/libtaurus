pub const NULLSPACE_ID: NamespaceId = NamespaceId::with_bytes([
    175, 19, 73, 185, 245, 249, 161, 166, 160, 64, 77, 234, 54, 220, 201, 73, 155, 203, 37, 201,
    173, 193, 18, 183, 204, 154, 147, 202, 228, 31, 50, 98,
]);

/// Unique identifier for a ['Namespace']
pub type NamespaceId = crate::hash::Hash;

/// Namespace label for transaction categorization
#[derive(Clone, Default, Debug)]
pub struct Namespace(String);

impl Namespace {
    /// Create a new ['Namespace'] instance
    pub fn new(name: &str) -> Namespace {
        Namespace(name.to_string())
    }

    /// Compute the ['NamespaceId'] for the given ['Namespace']
    pub fn id(&self) -> NamespaceId {
        blake3::hash(&self.0.as_bytes()).into()
    }
}

#[cfg(test)]
mod test {
    use crate::consensus::namespace::{self, Namespace, NamespaceId};

    #[test]
    fn id() {
        assert_eq!(Namespace::new("").id(), namespace::NULLSPACE_ID);
        assert_eq!(
            Namespace::new("Hello, World!").id(),
            NamespaceId::with_bytes([
                40, 138, 134, 167, 159, 32, 163, 214, 220, 205, 202, 119, 19, 190, 174, 209, 120,
                121, 130, 150, 189, 250, 121, 19, 250, 42, 98, 217, 114, 123, 248, 248
            ])
        );
    }
}
