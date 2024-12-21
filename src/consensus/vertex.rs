use super::{
    namespace::{Namespace, NamespaceId},
    transaction::TxRoot,
};
use crate::wire::{generated::proto, WireFormat};
use chrono::{DateTime, Utc};
use core::fmt;
use itertools::Itertools;
use serde_derive::{Deserialize, Serialize};
use std::{hash::Hash, result, sync::Arc};

/// Current revision of the vertex structure
pub const VERSION: u32 = 1;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("bad version (expected {0}, found {1})")]
    BadVersion(u32, u32),
    #[error("duplicate parents")]
    DuplicateParents,
    #[error("missing field: {0}")]
    MissingField(String),
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

/// Transaction vertex, representing a batch of transactions within the graph
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vertex {
    /// Revision number of the vertex structure
    pub version: u32,

    /// Height of this vertex in the DAG
    pub height: u64,

    /// Vertex parents ordered according to sequence of observation
    pub parents: Vec<VertexHash>,

    /// ID of the transaction namespace this vertex belongs to
    pub namespace_id: NamespaceId,

    /// Root hash of the transactions contained in this vertex
    pub root: TxRoot,

    /// The time at which we first observed this vertex
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
            root: TxRoot::default(),
            timestamp: Utc::now(),
        }
    }

    /// Assign new parents to the given vertex
    pub fn with_parents<'a, P>(mut self, parents: P) -> Result<Vertex>
    where
        P: IntoIterator<Item = &'a Arc<Vertex>>,
    {
        let (heights, hashes): (Vec<_>, Vec<_>) =
            parents.into_iter().map(|p| (p.height, p.hash())).unzip();
        if heights.is_empty() {
            Err(Error::MissingField("parents".to_string()))
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
            Err(Error::BadVersion(VERSION, self.version))
        } else if self.parents.is_empty() {
            Err(Error::MissingField("parents".to_string()))
        } else if self.parents.len() != self.parents.iter().unique().count() {
            Err(Error::DuplicateParents)
        } else {
            Ok(())
        }
    }

    /// Return a list of parent ordering [`Constraint`]s this [`Vertex`] commits to
    pub fn parent_constraints(&self) -> Constraints {
        Constraints::new(self.parents.clone())
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
                vertex
                    .namespace_id
                    .as_ref()
                    .ok_or(Error::MissingField("namespace".to_string()))?,
                check,
            )?,
            root: TxRoot::from_protobuf(
                vertex
                    .root
                    .as_ref()
                    .ok_or(Error::MissingField("root".to_string()))?,
                check,
            )?,
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
    height: u64,
    parents: Vec<String>,
    namespace_id: String,
    tx_root: String,
    timestamp: DateTime<Utc>,
}

impl From<&Vertex> for PrettyVertex {
    fn from(vertex: &Vertex) -> Self {
        PrettyVertex {
            version: vertex.version,
            height: vertex.height,
            parents: vertex.parents.iter().map(|p| p.to_hex()).collect(),
            namespace_id: vertex.namespace_id.to_hex(),
            tx_root: vertex.root.to_hex(),
            timestamp: vertex.timestamp,
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

/// Key used to map a constraint to the conflict set it belongs to
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConflictSetKey([u8; 64]);

/// A [`Constraint`] describes an ordred [`Vertex`] pair, and is used to reach consensus on the
/// total ordering of vertices in the graph. The left [`Vertex`] is constraint to come in sequence
/// before the right.
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Constraint(pub VertexHash, pub VertexHash);

impl Constraint {
    /// Compute a deterministic key that can be used to map any constraint to the same conflict set
    /// as its opposite.
    pub fn conflict_set_key(&self) -> ConflictSetKey {
        let (lt, gt) = if self.0 < self.1 {
            (self.0, self.1)
        } else {
            (self.1, self.0)
        };
        let mut ret = ConflictSetKey([0; 64]);
        ret.0[..32].copy_from_slice(lt.as_slice());
        ret.0[32..].copy_from_slice(gt.as_slice());
        ret
    }

    /// Return a [`Constraint`] for the opposite [`Vertex`] ordering, if any. In the case of a
    /// unity constraint, there is no opposite.
    pub fn conflict(&self) -> Option<Constraint> {
        if !self.is_unity() {
            Some(Constraint(self.1, self.0))
        } else {
            None
        }
    }

    /// Returns true if this is a unity constraint; i.e. it only references one vertex
    pub fn is_unity(&self) -> bool {
        self.0 == self.1
    }
}

impl fmt::Display for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Constraint({}, {})",
            self.0.to_short_hex(),
            self.1.to_short_hex()
        )
    }
}

/// An [`Iterator`] which iterates over every permutation of parents [`Constraint`]s a [`Vertex`]
/// commits to.
pub struct Constraints {
    vertices: Vec<VertexHash>,
    idx1: usize,
    idx2: usize,
}

impl Constraints {
    fn new(vertices: Vec<VertexHash>) -> Constraints {
        Constraints {
            vertices,
            idx1: 0,
            idx2: 1,
        }
    }
}

impl Iterator for Constraints {
    type Item = Constraint;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx1 + 1 >= self.vertices.len() {
            self.vertices.pop().map(|vhash| Constraint(vhash, vhash))
        } else {
            let constraint = Constraint(self.vertices[self.idx1], self.vertices[self.idx2]);
            self.idx2 += 1;
            if self.idx2 == self.vertices.len() {
                self.idx1 += 1;
                self.idx2 = self.idx1 + 1;
            }
            Some(constraint)
        }
    }
}

// Helper to build test vertices with unique hashes
pub fn make_rand_vertex<'a, P>(parents: P) -> Arc<Vertex>
where
    P: IntoIterator<Item = &'a Arc<Vertex>>,
{
    use rand::{distributions::Alphanumeric, Rng};

    let rand_namespace = Namespace::new(
        &rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(30)
            .map(char::from)
            .collect::<String>(),
    );
    Arc::new(
        Vertex::empty()
            .with_parents(parents)
            .unwrap()
            .in_namespace(rand_namespace)
            .unwrap(),
    )
}

#[cfg(test)]
mod test {
    use crate::{
        consensus::{
            namespace::{self, Namespace, NamespaceId},
            transaction::TxRoot,
        },
        vertex::{self, Constraint, Constraints, VERSION},
        Vertex, VertexHash, WireFormat,
    };
    use core::panic;
    use std::{assert_matches::assert_matches, sync::Arc};

    // Some reusable dummy hashes
    const H1: VertexHash = VertexHash::with_bytes([
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31,
    ]);
    const H2: VertexHash = VertexHash::with_bytes([
        10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119,
        120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131,
    ]);
    const H3: VertexHash = VertexHash::with_bytes([
        11, 11, 12, 13, 14, 15, 16, 17, 18, 19, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119,
        120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131,
    ]);
    const H4: VertexHash = VertexHash::with_bytes([
        12, 11, 12, 13, 14, 15, 16, 17, 18, 19, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119,
        120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131,
    ]);

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
            TxRoot::default(),
            "unexpected tx_root"
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
        let p1 = Arc::new(p1);
        let p2 = Arc::new(p2);
        let p3 = Arc::new(p3);
        match Vertex::empty().with_parents([]) {
            Err(vertex::Error::MissingField(field)) => assert_eq!(field, "parents".to_string()),
            _ => panic!("expected error"),
        }
        assert_matches!(
            Vertex::empty().with_parents([&p1, &p2, &p2]),
            Err(vertex::Error::DuplicateParents),
            "should error if duplicate parents provided"
        );
        assert_eq!(Vertex::empty().with_parents([&p1, &p2]).unwrap().height, 2,);
        assert_eq!(
            Vertex::empty()
                .with_parents([&p1, &p2, &p3])
                .unwrap()
                .height,
            101,
        );
        assert_eq!(Vertex::empty().with_parents([&p3]).unwrap().height, 101);
        assert_eq!(
            Vertex::empty()
                .with_parents([&p1, &p2, &p3])
                .unwrap()
                .height,
            101,
        );
        assert_eq!(
            Vertex::empty()
                .with_parents([&p1, &p2, &p3])
                .unwrap()
                .parents,
            [p1.hash(), p2.hash(), p3.hash()]
        );
        assert_eq!(
            Vertex::empty()
                .with_parents([&p3, &p1, &p2])
                .unwrap()
                .parents,
            [p3.hash(), p1.hash(), p2.hash()]
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
        assert_matches!(
            &vx.sanity_checks(),
            Err(vertex::Error::BadVersion(VERSION, 0))
        );
        vx.version = vertex::VERSION;
        match vx.sanity_checks() {
            Err(vertex::Error::MissingField(field)) => assert_eq!(field, "parents".to_string()),
            _ => panic!("expected error"),
        }
        vx.parents = vec![VertexHash::default(), VertexHash::default()];
        assert_matches!(&vx.sanity_checks(), Err(vertex::Error::DuplicateParents));
        vx.parents = vec![VertexHash::default()];
        let vx = vx.with_parents(vec![&Arc::new(Vertex::default())]).unwrap();
        assert_matches!(&vx.sanity_checks(), Ok(()));
    }

    #[test]
    fn parent_constraints() {
        let mut p1 = Vertex::empty();
        let mut p2 = Vertex::empty();
        let mut p3 = Vertex::empty();
        // Give each some unique data to result in unique parent hashes
        p1.height = 1;
        p2.height = 2;
        p3.height = 3;
        let p1 = Arc::new(p1);
        let p2 = Arc::new(p2);
        let p3 = Arc::new(p3);
        assert!(Vertex::empty()
            .with_parents(vec![&p1])
            .unwrap()
            .parent_constraints()
            .eq(vec![Constraint(p1.hash(), p1.hash())]));
        assert!(Vertex::empty()
            .with_parents(vec![&p1, &p2])
            .unwrap()
            .parent_constraints()
            .eq(vec![
                Constraint(p1.hash(), p2.hash()),
                Constraint(p2.hash(), p2.hash()),
                Constraint(p1.hash(), p1.hash()),
            ]));
        assert!(Vertex::empty()
            .with_parents(vec![&p1, &p2, &p3])
            .unwrap()
            .parent_constraints()
            .eq(vec![
                Constraint(p1.hash(), p2.hash()),
                Constraint(p1.hash(), p3.hash()),
                Constraint(p2.hash(), p3.hash()),
                Constraint(p3.hash(), p3.hash()),
                Constraint(p2.hash(), p2.hash()),
                Constraint(p1.hash(), p1.hash()),
            ]));
    }

    #[test]
    fn constraint_conflict_set_key() {
        assert_eq!(
            Constraint(H1, H2).conflict_set_key(),
            Constraint(H2, H1).conflict_set_key()
        );
        assert_eq!(
            Constraint(H3, H4).conflict_set_key(),
            Constraint(H4, H3).conflict_set_key()
        );
    }

    #[test]
    fn constraint_conflict() {
        assert_eq!(Constraint(H1, H2), Constraint(H1, H2));
        assert_ne!(Constraint(H1, H2), Constraint(H2, H1));
        assert_eq!(Constraint(H1, H1).conflict(), None);
        assert_eq!(Constraint(H1, H2).conflict(), Some(Constraint(H2, H1)));
        assert_eq!(Constraint(H3, H4).conflict(), Some(Constraint(H4, H3)));
    }

    #[test]
    fn unity_constraint() {
        assert_eq!(Constraint(H1, H1).is_unity(), true);
        assert_eq!(Constraint(H2, H1).is_unity(), false);
    }

    #[test]
    fn new_constraint_iter() {
        assert!(Constraints::new(vec![H1]).eq(vec![Constraint(H1, H1)]));
        assert!(Constraints::new(vec![H1, H2]).eq(vec![
            Constraint(H1, H2),
            Constraint(H2, H2),
            Constraint(H1, H1),
        ]));
        assert!(Constraints::new(vec![H3, H2]).eq(vec![
            Constraint(H3, H2),
            Constraint(H2, H2),
            Constraint(H3, H3),
        ]));
        assert!(Constraints::new(vec![H1, H2, H3]).eq(vec![
            Constraint(H1, H2),
            Constraint(H1, H3),
            Constraint(H2, H3),
            Constraint(H3, H3),
            Constraint(H2, H2),
            Constraint(H1, H1),
        ]));
        assert!(Constraints::new(vec![H1, H2, H3, H4]).eq(vec![
            Constraint(H1, H2),
            Constraint(H1, H3),
            Constraint(H1, H4),
            Constraint(H2, H3),
            Constraint(H2, H4),
            Constraint(H3, H4),
            Constraint(H4, H4),
            Constraint(H3, H3),
            Constraint(H2, H2),
            Constraint(H1, H1),
        ]));
    }
}
