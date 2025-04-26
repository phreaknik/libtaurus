use libtaurus::{
    consensus::{
        event::EventRoot,
        namespace::{self, Namespace, NamespaceId},
    },
    vertex, Vertex, VertexHash,
};
use std::assert_matches::assert_matches;

#[test]
fn build() {
    assert_eq!(
        Vertex::build().version,
        vertex::VERSION,
        "unexpected version number"
    );
    assert!(Vertex::build().parents.is_empty(), "unexpected parents");
    assert_eq!(
        Vertex::build().namespace_id,
        NamespaceId::default(),
        "unexpected namespace_id"
    );
    assert_eq!(
        Vertex::build().event_root,
        EventRoot::default(),
        "unexpected event_root"
    );
}

#[test]
fn with_parents() {
    assert_eq!(
        Vertex::build()
            .with_parents(vec![VertexHash::default(), VertexHash::default()])
            .unwrap()
            .parents,
        vec![VertexHash::default(), VertexHash::default()],
        "unexpected parents"
    );
    assert_matches!(
        Vertex::build().with_parents(Vec::new()),
        Err(vertex::Error::NoParents),
        "should error with empty parents"
    );
}

#[test]
fn in_namespace() {
    assert_eq!(
        Vertex::build()
            .in_namespace(Namespace::default())
            .unwrap()
            .namespace_id,
        namespace::NULLSPACE_ID,
        "unexpected namespace_id"
    );
}

#[test]
fn sanity_checks() {
    let mut v = Vertex::build();
    v.version = 0;
    assert_matches!(v.sanity_checks(), Err(vertex::Error::BadVersion(0)));
    v.version = vertex::VERSION;
    assert_matches!(v.sanity_checks(), Err(vertex::Error::NoParents));
    v = v.with_parents(vec![VertexHash::default()]).unwrap();
    assert_matches!(v.sanity_checks(), Ok(()));
}
