use super::{
    event::EventRoot,
    namespace::{Namespace, NamespaceId},
};
use crate::wire::{generated::proto, WireFormat};
use chrono::{DateTime, Utc};
use itertools::Itertools;
use serde_derive::{Deserialize, Serialize};
use std::result;

/// Current revision of the vertex structure
pub const VERSION: u32 = 1;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("bad version")]
    BadVersion(u32),
    #[error("duplicate parents")]
    DuplicateParents,
    #[error("no namespace")]
    NoNamespace,
    #[error("no parents")]
    NoParents,
    #[error("no root")]
    NoRoot,
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
    pub root: EventRoot,

    /// The time which we first observed this vertex
    pub timestamp: DateTime<Utc>,
}

impl Vertex {
    /// Create an empty [`Vertex`]
    pub fn empty() -> Vertex {
        Vertex {
            version: VERSION,
            height: 0,
            parents: Vec::new(),
            namespace_id: NamespaceId::default(),
            root: EventRoot::default(),
            timestamp: Utc::now(),
        }
    }

    /// Assign new parents to the given vertex
    pub fn with_parents<'a, P>(mut self, parents: P) -> Result<Vertex>
    where
        P: IntoIterator<Item = &'a Vertex>,
    {
        let (heights, hashes): (Vec<_>, Vec<_>) =
            parents.into_iter().map(|p| (p.height, p.hash())).unzip();
        if heights.is_empty() {
            Err(Error::NoParents)
        } else if hashes.len() != hashes.iter().unique().count() {
            Err(Error::DuplicateParents)
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
        } else if self.parents.len() != self.parents.iter().unique().count() {
            Err(Error::DuplicateParents)
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
        Ok(proto::Vertex {
            version: self.version,
            height: self.height,
            parents: self
                .parents
                .iter()
                .map(|p| p.to_protobuf(check))
                .try_collect()?,
            namespace_id: Some(self.namespace_id.to_protobuf(check)?),
            root: Some(self.root.to_protobuf(check)?),
        })
    }

    fn from_protobuf(vertex: &proto::Vertex, check: bool) -> Result<Vertex> {
        let vx = Vertex {
            version: vertex.version,
            height: vertex.height,
            parents: vertex
                .parents
                .iter()
                .map(|p| VertexHash::from_protobuf(p, check))
                .try_collect()?,
            namespace_id: NamespaceId::from_protobuf(
                vertex.namespace_id.as_ref().ok_or(Error::NoNamespace)?,
                check,
            )?,
            root: EventRoot::from_protobuf(vertex.root.as_ref().ok_or(Error::NoRoot)?, check)?,
            timestamp: Utc::now(),
        };
        // Optionally perform sanity checks
        if check {
            vx.sanity_checks()?;
        }
        Ok(vx)
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
            event_root: vertex.root.to_hex(),
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
        if self.vertices.len() < 2 {
            None
        } else {
            if self.idx1 == self.vertices.len() - 1 {
                None
            } else {
                let pair = VertexPair::new(self.vertices[self.idx1], self.vertices[self.idx2]);
                self.idx2 += 1;
                if self.idx2 == self.vertices.len() {
                    self.idx1 += 1;
                    self.idx2 = self.idx1 + 1;
                }
                Some(pair)
            }
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
        vertex::{self, VertexPair, VertexPairs},
        Vertex, VertexHash, WireFormat,
    };
    use std::assert_matches::assert_matches;

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
            Vertex::empty().root,
            EventRoot::default(),
            "unexpected event_root"
        );
    }

    #[test]
    fn with_parents() {
        let mut p1 = Vertex::empty();
        let mut p2 = Vertex::empty();
        let mut p3 = Vertex::empty();
        p1.height = 0;
        p2.height = 1;
        p3.height = 100;
        assert_matches!(
            Vertex::empty().with_parents(Vec::new()),
            Err(vertex::Error::NoParents),
            "should error if no parents provided"
        );
        assert_matches!(
            Vertex::empty().with_parents(vec![&p1, &p2, &p2]),
            Err(vertex::Error::DuplicateParents),
            "should error if duplicate parents provided"
        );
        assert_eq!(
            Vertex::empty().with_parents(vec![&p1, &p2]).unwrap().height,
            2,
        );
        assert_eq!(
            Vertex::empty()
                .with_parents(vec![&p1, &p2, &p3])
                .unwrap()
                .height,
            101,
        );
        assert_eq!(Vertex::empty().with_parents(vec![&p3]).unwrap().height, 101);
        assert_eq!(
            Vertex::empty()
                .with_parents(vec![&p1, &p2, &p3])
                .unwrap()
                .height,
            101,
        );
        assert_eq!(
            Vertex::empty()
                .with_parents(vec![&p1, &p2, &p3])
                .unwrap()
                .parents,
            vec![p1.hash(), p2.hash(), p3.hash()]
        );
        assert_eq!(
            Vertex::empty()
                .with_parents(vec![&p3, &p1, &p2])
                .unwrap()
                .parents,
            vec![p3.hash(), p1.hash(), p2.hash()]
        );
    }

    #[test]
    fn in_namespace() {
        assert_eq!(
            Vertex::empty()
                .in_namespace(Namespace::default())
                .unwrap()
                .namespace_id,
            namespace::NULLSPACE_ID,
            "unexpected default namespace_id"
        );
        assert_eq!(
            Vertex::empty()
                .in_namespace(Namespace::new("helloworld"))
                .unwrap()
                .namespace_id,
            Namespace::new("helloworld").id(),
            "unexpected namespace_id for helloworld"
        );
    }

    #[test]
    fn sanity_checks() {
        let mut vx = Vertex::default();
        assert_matches!(&vx.sanity_checks(), Err(vertex::Error::BadVersion(0)));
        vx.version = vertex::VERSION;
        assert_matches!(&vx.sanity_checks(), Err(vertex::Error::NoParents));
        vx.parents = vec![VertexHash::default(), VertexHash::default()];
        assert_matches!(&vx.sanity_checks(), Err(vertex::Error::DuplicateParents));
        vx.parents = vec![VertexHash::default()];
        let vx = vx.with_parents(vec![&Vertex::default()]).unwrap();
        assert_matches!(&vx.sanity_checks(), Ok(()));
    }

    #[test]
    fn parent_pairs() {
        let mut p1 = Vertex::empty();
        let mut p2 = Vertex::empty();
        let mut p3 = Vertex::empty();
        // Give each some unique data to result in unique parent hashes
        p1.height = 1;
        p2.height = 2;
        p3.height = 3;
        assert!(Vertex::empty()
            .with_parents(vec![&p1])
            .unwrap()
            .parent_pairs()
            .eq(vec![]));
        assert!(Vertex::empty()
            .with_parents(vec![&p1, &p2])
            .unwrap()
            .parent_pairs()
            .eq(vec![VertexPair::new(p1.hash(), p2.hash())]));
        assert!(Vertex::empty()
            .with_parents(vec![&p1, &p2, &p3])
            .unwrap()
            .parent_pairs()
            .eq(vec![
                VertexPair::new(p1.hash(), p2.hash()),
                VertexPair::new(p1.hash(), p3.hash()),
                VertexPair::new(p2.hash(), p3.hash()),
            ]));
    }

    #[test]
    fn new_pair() {
        let h1 = VertexHash::with_bytes([
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31,
        ]);
        let h2 = VertexHash::with_bytes([
            10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 110, 111, 112, 113, 114, 115, 116, 117, 118,
            119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131,
        ]);
        assert_eq!(VertexPair::new(h1, h2), VertexPair::new(h2, h1));
        assert_eq!(VertexPair::new(h1, h2), VertexPair(h1, h2));
    }

    #[test]
    fn new_pair_iter() {
        let h1 = VertexHash::with_bytes([
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31,
        ]);
        let h2 = VertexHash::with_bytes([
            10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 110, 111, 112, 113, 114, 115, 116, 117, 118,
            119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131,
        ]);
        let h3 = VertexHash::with_bytes([
            11, 11, 12, 13, 14, 15, 16, 17, 18, 19, 110, 111, 112, 113, 114, 115, 116, 117, 118,
            119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131,
        ]);
        let h4 = VertexHash::with_bytes([
            12, 11, 12, 13, 14, 15, 16, 17, 18, 19, 110, 111, 112, 113, 114, 115, 116, 117, 118,
            119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131,
        ]);
        assert!(VertexPairs::new(&vec![h1]).eq(vec![]));
        assert!(VertexPairs::new(&vec![h1, h2]).eq(vec![VertexPair::new(h1, h2)]));
        assert!(VertexPairs::new(&vec![h3, h2]).eq(vec![VertexPair::new(h2, h3)]));
        assert!(VertexPairs::new(&vec![h1, h2, h3]).eq(vec![
            VertexPair::new(h1, h2),
            VertexPair::new(h1, h3),
            VertexPair::new(h2, h3),
        ]));
        assert!(VertexPairs::new(&vec![h1, h2, h3, h4]).eq(vec![
            VertexPair::new(h1, h2),
            VertexPair::new(h1, h3),
            VertexPair::new(h1, h4),
            VertexPair::new(h2, h3),
            VertexPair::new(h2, h4),
            VertexPair::new(h3, h4),
        ]));
    }
}
