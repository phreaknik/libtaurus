use libtaurus::consensus::namespace::{self, Namespace, NamespaceId};

#[test]
fn id() {
    assert_eq!(Namespace::new("").id(), namespace::NULLSPACE_ID);
    assert_eq!(
        Namespace::new("Hello, World!").id(),
        NamespaceId::with_bytes([
            40, 138, 134, 167, 159, 32, 163, 214, 220, 205, 202, 119, 19, 190, 174, 209, 120, 121,
            130, 150, 189, 250, 121, 19, 250, 42, 98, 217, 114, 123, 248, 248
        ])
    );
}
