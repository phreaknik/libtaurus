pub const NULLSPACE_ID: NamespaceId = NamespaceId::with_bytes([
    175, 19, 73, 185, 245, 249, 161, 166, 160, 64, 77, 234, 54, 220, 201, 73, 155, 203, 37, 201,
    173, 193, 18, 183, 204, 154, 147, 202, 228, 31, 50, 98,
]);

/// Unique identifier for a ['Namespace']
pub type NamespaceId = crate::hash::Hash;

/// Event namespace
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
