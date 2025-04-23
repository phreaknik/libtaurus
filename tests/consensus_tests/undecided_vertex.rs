use cached::{Cached, TimedCache};
use cordelia::{
    consensus::{vertex, UndecidedVertex},
    Block, BlockHash, Vertex, VertexHash,
};
use itertools::Itertools;
use std::{assert_matches::assert_matches, collections::HashMap, sync::Arc};
use tracing_mutex::stdsync::TracingRwLock;

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
    let genesis_hash = VertexHash::with_bytes([
        33, 180, 159, 199, 73, 80, 3, 76, 173, 198, 97, 18, 238, 185, 30, 114, 45, 40, 216, 37,
        158, 44, 255, 255, 191, 100, 71, 154, 244, 153, 180, 31,
    ]);
    let mut undecided_blocks = TimedCache::with_lifespan_and_refresh(1000, true);
    let mut undecided_vertices = TimedCache::with_lifespan_and_refresh(1000, true);
    let parent1 = UndecidedVertex {
        inner: Arc::new(Vertex::new_slim(
            BlockHash::default(),
            vec![VertexHash::with_bytes([
                100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100,
                100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100,
            ])],
        )),
        height: 100,
        undecided_parents: HashMap::new(),
        known_children: HashMap::new(),
        strongly_preferred: true,
        chit: 0,
        confidence: 0,
    };
    let parent2 = UndecidedVertex {
        inner: Arc::new(Vertex::new_slim(
            BlockHash::default(),
            vec![VertexHash::with_bytes([
                200, 200, 200, 200, 200, 200, 200, 200, 200, 200, 200, 200, 200, 200, 200, 200,
                200, 200, 200, 200, 200, 200, 200, 200, 200, 200, 200, 200, 200, 200, 200, 200,
            ])],
        )),
        height: 107,
        undecided_parents: HashMap::new(),
        known_children: HashMap::new(),
        strongly_preferred: true,
        chit: 0,
        confidence: 0,
    };
    let inner = Arc::new(Vertex::new_full(Arc::new(
        Block::default().with_parents(vec![parent1.hash(), parent2.hash()]),
    )));
    undecided_vertices.cache_set(
        parent1.hash(),
        Arc::new(TracingRwLock::new(parent1.clone())),
    );
    // Test with one parent already decided
    assert_matches!(
        UndecidedVertex::new(
            genesis_hash,
            inner.clone(),
            &mut undecided_blocks,
            &mut undecided_vertices,
            false
        ),
        Err(vertex::Error::ParentsAlreadyDecided)
    );
    // Add the missing parent to the undecided set and try again
    undecided_vertices.cache_set(
        parent2.hash(),
        Arc::new(TracingRwLock::new(parent2.clone())),
    );
    // Test ok, strongly preferred
    let new = UndecidedVertex::new(
        genesis_hash,
        inner.clone(),
        &mut undecided_blocks,
        &mut undecided_vertices,
        true,
    )
    .unwrap();
    assert_eq!(new.inner, inner);
    assert_eq!(new.height, 108);
    assert!(new
        .undecided_parents
        .keys()
        .sorted()
        .eq(vec![parent1.hash(), parent2.hash()].iter().sorted()));
    assert_eq!(new.strongly_preferred, true);
    assert_eq!(new.chit, 0);
    assert_eq!(new.confidence, 0);
    // Test ok, not preferred
    let new = UndecidedVertex::new(
        genesis_hash,
        inner.clone(),
        &mut undecided_blocks,
        &mut undecided_vertices,
        false,
    )
    .unwrap();
    assert_eq!(new.inner, inner);
    assert_eq!(new.height, 108);
    assert!(new
        .undecided_parents
        .keys()
        .sorted()
        .eq(vec![parent1.hash(), parent2.hash()].iter().sorted()));
    assert_eq!(new.strongly_preferred, false);
    assert_eq!(new.chit, 0);
    assert_eq!(new.confidence, 0);
    // Test ok, with an unpreferred parent
    let mut parent3 = parent2.clone();
    parent3.strongly_preferred = false;
    undecided_vertices.cache_set(
        parent3.hash(),
        Arc::new(TracingRwLock::new(parent3.clone())),
    );
    let inner = Arc::new(Vertex::new_full(Arc::new(
        Block::default().with_parents(vec![parent1.hash(), parent2.hash(), parent3.hash()]),
    )));
    let new = UndecidedVertex::new(
        genesis_hash,
        inner.clone(),
        &mut undecided_blocks,
        &mut undecided_vertices,
        true,
    )
    .unwrap();
    assert_eq!(new.inner, inner);
    assert_eq!(new.height, 108);
    assert!(new
        .undecided_parents
        .keys()
        .sorted()
        .eq(vec![parent1.hash(), parent2.hash()].iter().sorted()));
    assert_eq!(new.strongly_preferred, false);
    assert_eq!(new.chit, 0);
    assert_eq!(new.confidence, 0);
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
