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

    /// Height of this vertex in the DAG
    pub height: u64,

    /// Vertex parents in ascending order of sequence (SESAME parent ordering)
    pub parents: Vec<VertexHash>,

    /// ID of the event namespace this vertex belongs to
    pub namespace_id: NamespaceId,

    /// Root hash of the events contained in this vertex
    pub event_root: EventRoot,
}

impl Vertex {
    /// Create an empty [`Vertex`]
    pub fn empty() -> Vertex {
        Vertex {
            version: VERSION,
            height: 0,
            parents: Vec::new(),
            namespace_id: NamespaceId::default(),
            event_root: EventRoot::default(),
        }
    }

    /// Assign new parents to the given vertex
    pub fn with_parents<P>(mut self, parents: P) -> Result<Vertex>
    where
        P: IntoIterator<Item = Vertex>,
    {
        let (heights, hashes): (Vec<_>, Vec<_>) =
            parents.into_iter().map(|p| (p.height, p.hash())).unzip();
        if heights.is_empty() {
            Err(Error::NoParents)
        } else {
            self.height = 1 + heights.into_iter().max().unwrap();
            self.parents = hashes;
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

    /// Return a list of [`VertexPair`]s for every permutation of pairs of this vertex's parents
    pub fn parent_pairs<'a>(&'a self) -> VertexPairs<'a> {
        VertexPairs::new(&self.parents)
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

/// A pair of [`Vertex`]s used to build parent ordering constraints
#[derive(Clone, Default, Debug, PartialEq, Eq, Hash)]
pub struct VertexPair(pub VertexHash, pub VertexHash);

impl VertexPair {
    /// Construct a new [`VertexPair`] from two [`VertexHash`]s.
    pub fn new(vx1: VertexHash, vx2: VertexHash) -> VertexPair {
        if vx1 < vx2 {
            VertexPair(vx1, vx2)
        } else {
            VertexPair(vx2, vx1)
        }
    }
}

/// An [`Iterator`] which iterates over every permutation of pairs of parents of a [`Vertex`].
pub struct VertexPairs<'a> {
    vertices: &'a Vec<VertexHash>,
    idx1: usize,
    idx2: usize,
}

impl<'a> VertexPairs<'a> {
    fn new(vertices: &'a Vec<VertexHash>) -> VertexPairs<'a> {
        VertexPairs {
            vertices,
            idx1: 0,
            idx2: 1,
        }
    }
}

impl<'a> Iterator for VertexPairs<'a> {
    type Item = VertexPair;

    fn next(&mut self) -> Option<Self::Item> {
        let pair = VertexPair::new(self.vertices[self.idx1], self.vertices[self.idx2]);
        self.idx2 += 1;
        if self.idx2 == self.vertices.len() {
            self.idx1 += 1;
            self.idx2 = self.idx1 + 1;
        }
        if self.idx1 == self.vertices.len() {
            None
        } else {
            Some(pair)
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{
        consensus::{
            event::EventRoot,
            namespace::{self, Namespace, NamespaceId},
        },
        vertex, Vertex,
    };

    #[test]
    fn empty() {
        assert_eq!(
            Vertex::empty().version,
            vertex::VERSION,
            "unexpected version number"
        );
        assert!(Vertex::empty().parents.is_empty(), "unexpected parents");
        assert_eq!(
            Vertex::empty().namespace_id,
            NamespaceId::default(),
            "unexpected namespace_id"
        );
        assert_eq!(
            Vertex::empty().event_root,
            EventRoot::default(),
            "unexpected event_root"
        );
    }

    #[test]
    fn with_parents() {
        todo!()
    }

    #[test]
    fn in_namespace() {
        assert_eq!(
            Vertex::empty()
                .in_namespace(Namespace::default())
                .unwrap()
                .namespace_id,
            namespace::NULLSPACE_ID,
            "unexpected namespace_id"
        );
    }

    #[test]
    fn sanity_checks() {
        todo!()
    }

    #[test]
    fn parent_pairs() {
        todo!()
    }

    #[test]
    fn new_pair() {
        todo!()
    }

    #[test]
    fn new_pair_iter() {
        todo!()
    }
}
