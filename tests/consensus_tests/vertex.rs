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
        parents: block.parents.clone(),
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
        parents: parents.clone(),
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
        parents: Vec::new(),
        block: Some(Arc::new(block.clone())),
    };
    assert_eq!(expected, Vertex::genesis(block));
}

#[test]
fn sanity_checks() {
    let mut v = Vertex {
        version: 999,                // Bad version
        bhash: BlockHash::default(), // Bad hash
        parents: vec![VertexHash::default()],
        block: Some(Arc::new(
            Block::default().with_parents(vec![VertexHash::default()]),
        )),
    };
    assert_matches!(v.sanity_checks(), Err(vertex::Error::BadVersion(999)));
    v.version = 0;
    assert_matches!(v.sanity_checks(), Err(vertex::Error::BadBlockHash));
    v.bhash = v.block.as_ref().unwrap().hash();
    assert!(v.sanity_checks().is_ok()); // Valid full-vertex
    v.block = None;
    assert!(v.sanity_checks().is_ok()); // Valid slim-vertex
    v.parents = vec![VertexHash::default(), VertexHash::default()];
    assert_matches!(v.sanity_checks(), Err(vertex::Error::RepeatedParents));
    v.parents = Vec::new();
    assert_matches!(v.sanity_checks(), Err(vertex::Error::EmptyParents));
}
