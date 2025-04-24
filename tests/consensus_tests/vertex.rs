use chrono::Utc;
use cordelia::{
    consensus::vertex, params::MIN_DIFFICULTY, util::Randomizer, Block, BlockHash, Vertex,
    VertexHash,
};
use libp2p::{multihash::Multihash, PeerId};
use std::{assert_matches::assert_matches, sync::Arc};

#[test]
fn new_full() {
    let block = Arc::new(Block::random());
    let expected = Vertex {
        version: vertex::VERSION,
        bhash: block.hash(),
        parents: None,
        block: Some(block.clone()),
    };
    assert_eq!(expected, Vertex::new_full(block));
}

#[test]
fn new_slim() {
    let bhash = BlockHash::default();
    let parents = vec![VertexHash::default()];
    let expected = Vertex {
        version: vertex::VERSION,
        bhash,
        parents: Some(parents.clone()),
        block: None,
    };
    assert_eq!(expected, Vertex::new_slim(bhash, parents));
}

#[test]
fn genesis() {
    let block = Block {
        version: 0,
        difficulty: MIN_DIFFICULTY,
        miner: PeerId::from_multihash(Multihash::default()).unwrap(),
        parents: Vec::new(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        time: Utc::now(),
        nonce: 0,
    };
    let expected = Vertex {
        version: 0,
        bhash: block.hash(),
        parents: None,
        block: Some(Arc::new(block.clone())),
    };
    assert_eq!(expected, Vertex::genesis(block));
}

#[test]
fn with_block() {
    let parents = vec![VertexHash::default()];
    let block = Arc::new(Block::default().with_parents(parents.clone()));
    let bhash = block.hash();
    let slim = Vertex::new_slim(bhash, parents);
    let mut expected = slim.clone();
    expected.parents = None;
    expected.block = Some(block.clone());
    assert_eq!(expected, slim.with_block(block));
}

#[test]
fn with_new_parents() {
    let parents = vec![VertexHash::default()];
    let block = Arc::new(Block::default().with_parents(parents.clone()));
    let full = Vertex::new_full(block);
    let mut expected = full.clone();
    expected.parents = Some(parents.clone());
    expected.block = None;
    assert_eq!(expected, full.with_new_parents(parents));
}

#[test]
fn parents() {
    let orig_parents = vec![VertexHash::with_bytes([
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ])];
    let block = Arc::new(Block::default().with_parents(orig_parents.clone()));
    let full = Vertex::new_full(block);
    assert_eq!(&orig_parents, full.parents());

    let new_parents = vec![
        VertexHash::with_bytes([
            0, 82, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            77, 123, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        ]),
        VertexHash::with_bytes([
            99, 82, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            71, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 2,
        ]),
    ];
    let slim = full.clone().with_new_parents(new_parents.clone());
    assert_eq!(&new_parents, slim.parents());
}

#[test]
fn slim() {
    let parents = vec![
        VertexHash::with_bytes([
            0, 82, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            77, 123, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        ]),
        VertexHash::with_bytes([
            99, 82, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            71, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 2,
        ]),
    ];
    let block = Arc::new(Block::default().with_parents(parents.clone()));
    let full = Vertex::new_full(block.clone());
    let slim = full.clone().with_new_parents(parents);
    assert_eq!((Some(block), slim), full.slim());
}

#[test]
fn sanity_checks() {
    let mut v = Vertex {
        version: 999,                // Bad version
        bhash: BlockHash::default(), // Bad hash
        parents: Some(Vec::new()),   // Redundant parents
        block: Some(Arc::new(
            Block::default().with_parents(vec![VertexHash::default()]),
        )),
    };
    assert_matches!(v.sanity_checks(), Err(vertex::Error::BadVersion(999)));
    v.version = 0;
    assert_matches!(v.sanity_checks(), Err(vertex::Error::BadBlockHash));
    v.bhash = v.block.as_ref().unwrap().hash();
    assert_matches!(v.sanity_checks(), Err(vertex::Error::RedundantParents));
    v.parents = None;
    assert!(v.sanity_checks().is_ok()); // Valid full-vertex
    v.block = None;
    assert_matches!(v.sanity_checks(), Err(vertex::Error::EmptyParents));
    v.parents = Some(vec![VertexHash::default(), VertexHash::default()]);
    assert_matches!(v.sanity_checks(), Err(vertex::Error::RepeatedParents));
    v.parents = Some(vec![VertexHash::default()]);
    assert!(v.sanity_checks().is_ok()); // Valid slim-vertex
}
