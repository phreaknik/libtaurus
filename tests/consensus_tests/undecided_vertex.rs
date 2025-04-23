use cordelia::{consensus::UndecidedVertex, Block, Vertex, VertexHash};
use std::sync::Arc;

#[test]
fn genesis() {
    let block = Arc::new(Block::default());
    let vertex = Arc::new(Vertex::new_full(block));
    let genesis = UndecidedVertex::genesis(vertex.clone());
    assert_eq!(genesis.inner, vertex);
    assert_eq!(genesis.height, 0);
    assert!(genesis.undecided_parents.is_empty());
    assert!(genesis.known_children.is_empty());
    assert_eq!(genesis.strongly_preferred, true);
    assert_eq!(genesis.chit, 1);
    assert_eq!(genesis.confidence, usize::MAX);
}

#[test]
fn new() {
    todo!();
}

#[test]
fn hash() {
    let block = Arc::new(Block::default());
    let vertex = Arc::new(Vertex::new_full(block));
    let genesis = UndecidedVertex::genesis(vertex.clone());
    assert_eq!(
        genesis.hash(),
        VertexHash::with_bytes([
            33, 180, 159, 199, 73, 80, 3, 76, 173, 198, 97, 18, 238, 185, 30, 114, 45, 40, 216, 37,
            158, 44, 255, 255, 191, 100, 71, 154, 244, 153, 180, 31
        ])
    );
}
