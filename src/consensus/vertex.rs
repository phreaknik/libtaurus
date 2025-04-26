use super::{
    event::EventRoot,
    namespace::{Namespace, NamespaceId},
};
use crate::wire::{generated::proto, WireFormat};
use serde_derive::{Deserialize, Serialize};
use std::result;

/// Current revision of the vertex structure
pub const VERSION: u32 = 1;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("bad version")]
    BadVersion(u32),
    #[error("no parents")]
    NoParents,
    #[error(transparent)]
    ProstDecode(#[from] prost::DecodeError),
    #[error(transparent)]
    ProstEncode(#[from] prost::EncodeError),
    #[error(transparent)]
    Hash(#[from] crate::hash::Error),
}
type Result<T> = result::Result<T, Error>;

/// Type alias for vertex hashes
pub type VertexHash = crate::hash::Hash;

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct Vertex {
    /// Revision number of the vertex structure
    pub version: u32,

    /// Vertex parents in ascending order of sequence (SESAME parent ordering)
    pub parents: Vec<VertexHash>,

    /// ID of the event namespace this vertex belongs to
    pub namespace_id: NamespaceId,

    /// Root hash of the events contained in this vertex
    pub event_root: EventRoot,
}

impl Vertex {
    /// Start building a new vertex
    pub fn build() -> Vertex {
        Vertex {
            version: VERSION,
            parents: Vec::new(),
            namespace_id: NamespaceId::default(),
            event_root: EventRoot::default(),
        }
    }

    /// Assign new parents to the given vertex
    pub fn with_parents(mut self, parents: Vec<VertexHash>) -> Result<Vertex> {
        if parents.is_empty() {
            Err(Error::NoParents)
        } else {
            self.parents = parents;
            Ok(self)
        }
    }

    /// Assign this vertex to a namespace
    pub fn in_namespace(mut self, namespace: Namespace) -> Result<Vertex> {
        self.namespace_id = namespace.id();
        Ok(self)
    }

    /// Make sure the vertex passes all basic sanity checks
    pub fn sanity_checks(&self) -> Result<()> {
        if self.version != VERSION {
            Err(Error::BadVersion(self.version))
        } else if self.parents.is_empty() {
            Err(Error::NoParents)
        } else {
            Ok(())
        }
    }
}

impl<'a> WireFormat<'a, proto::Vertex> for Vertex {
    type Error = Error;

    fn to_protobuf(&self, check: bool) -> Result<proto::Vertex> {
        // Optionally perform sanity checks
        if check {
            self.sanity_checks()?;
        }
        todo!();
    }

    fn from_protobuf(_vertex: &proto::Vertex, _check: bool) -> Result<Vertex> {
        todo!();
        // Optionally perform sanity checks
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PrettyVertex {
    version: u32,
    parents: Vec<String>,
    namespace_id: String,
    event_root: String,
}

impl From<&Vertex> for PrettyVertex {
    fn from(vertex: &Vertex) -> Self {
        PrettyVertex {
            version: vertex.version,
            parents: vertex.parents.iter().map(|p| p.to_hex()).collect(),
            namespace_id: vertex.namespace_id.to_hex(),
            event_root: vertex.event_root.to_hex(),
        }
    }
}

impl std::fmt::Display for Vertex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_string_pretty(&PrettyVertex::from(self)).unwrap()
        )
    }
}
