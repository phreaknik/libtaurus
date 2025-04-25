use crate::consensus_tests::util::dag_runner::DagTestRunner;
use chrono::Duration;
use cordelia::{avalanche, consensus::block, Vertex, VertexHash, WireFormat};
use std::{assert_matches::assert_matches, iter::once, sync::Arc};

#[test]
fn genesis_hash() {
    let dag = DagTestRunner::new_full_vertex_runner().dag;
    assert_eq!(
        dag.genesis_hash(),
        VertexHash::with_bytes([
            239, 178, 84, 85, 32, 3, 79, 221, 101, 27, 231, 33, 138, 85, 173, 77, 7, 38, 159, 76,
            152, 14, 210, 162, 196, 242, 167, 110, 170, 31, 115, 144
        ])
    );
}

#[test]
fn try_insert_block() {
    let mut r = DagTestRunner::new_full_vertex_runner();
    let mut bad_nonce = false;
    let mut block = r.build_test_block(r.dag.get_frontier().unwrap(), 0);
    assert_matches!(
        r.dag.try_insert_block(Arc::new(block.clone()), false),
        Err(avalanche::Error::Block(block::Error::InvalidPoW))
    );
    block.nonce = 570068822920606640;
    bad_nonce |= r.mine_test_block(&mut block);
    // Append should succeed
    assert_matches!(
        r.dag.try_insert_block(Arc::new(block.clone()), false),
        Ok(())
    );
    block.parents.push(VertexHash::default()); // Insert missing parent
    block.nonce = 8730637501828490078;
    bad_nonce |= r.mine_test_block(&mut block);
    assert_matches!(
        r.dag.try_insert_block(Arc::new(block), false),
        Err(avalanche::Error::Waiting)
    );
    // TODO: test broadcast decision if dag still does P2P stuff
    assert_eq!(bad_nonce, false, "Generated blocks have incorrect nonces");
}

fn try_insert_basic(mut r: DagTestRunner) {
    r.build_test_vertices([
        ("v1_0", 14337490726892089899, vec!["genesis"]),
        ("v1_1", 16012302412638312007, vec!["genesis"]),
        ("v2_0", 06720339079374117241, vec!["v1_0"]),
        ("v3_0", 12563117955961592819, vec!["v2_0"]),
        ("v3_1", 09660934870233600764, vec!["v2_0", "v1_1"]),
        ("v3_2", 14749812702066963736, vec!["v1_0", "v1_1"]),
        ("v4_0", 14143367919018540815, vec!["v3_0", "v3_1", "v3_2"]),
    ]);

    // Should insert the full list successfully
    assert_matches!(
        r.try_insert_vertices(["v1_0", "v1_1", "v2_0", "v3_0", "v3_1", "v3_2", "v4_0",]),
        Ok(()),
    );
    assert!(r.frontier_matches(["v4_0"]));
}

#[test]
fn try_insert_basic_full_vertex() {
    try_insert_basic(DagTestRunner::new_full_vertex_runner())
}

#[test]
fn try_insert_basic_slim_vertex() {
    try_insert_basic(DagTestRunner::new_slim_vertex_runner())
}

fn try_insert_with_retries(mut r: DagTestRunner) {
    r.build_test_vertices([
        ("v1_0", 14337490726892089899, vec!["genesis"]),
        ("v1_1", 16012302412638312007, vec!["genesis"]),
        ("v2_0", 06720339079374117241, vec!["v1_0"]),
        ("v3_0", 12563117955961592819, vec!["v2_0"]),
        ("v3_1", 09660934870233600764, vec!["v2_0", "v1_1"]),
        ("v3_2", 14749812702066963736, vec!["v1_0", "v1_1"]),
        ("v4_0", 14143367919018540815, vec!["v3_0", "v3_1", "v3_2"]),
    ]);

    // Should insert up to the omitted vertex ("v3_0"), but return Err(Waiting) for the
    // remaining vertices
    assert_matches!(
        r.try_insert_vertices(["v1_0", "v1_1", "v2_0", "v3_1", "v3_2", "v4_0",]),
        Err(avalanche::Error::Waiting)
    );
    assert!(r.frontier_matches(["v3_1", "v3_2"]));

    // Insert the removed vertex. Should succeed as well as all previously waiting vertices.
    assert_matches!(r.try_insert_vertices(["v3_0"]), Ok(()));
    assert!(r.frontier_matches(["v4_0"]));
}

#[test]
fn try_insert_with_retries_full_vertex() {
    try_insert_with_retries(DagTestRunner::new_full_vertex_runner())
}

#[test]
fn try_insert_with_retries_slim_vertex() {
    try_insert_with_retries(DagTestRunner::new_slim_vertex_runner())
}

fn try_insert_duplicates(mut r: DagTestRunner) {
    r.build_test_vertices([
        ("v1_0", 14337490726892089899, vec!["genesis"]),
        ("v1_1", 16012302412638312007, vec!["genesis"]),
        ("v2_0", 06720339079374117241, vec!["v1_0"]),
        ("v3_0", 12563117955961592819, vec!["v2_0"]),
    ]);

    // Should insert the full list successfully
    assert_matches!(
        r.try_insert_vertices(["v1_0", "v1_1", "v2_0", "v3_0"]),
        Ok(()),
    );
    assert!(r.frontier_matches(["v3_0", "v1_1"]));
    // Should fail to reinsert a vertex
    assert_matches!(
        r.try_insert_vertices(["v1_0"]),
        Err(avalanche::Error::DuplicateInsertion),
    );
    assert!(r.frontier_matches(["v3_0", "v1_1"]));
}

#[test]
fn try_insert_duplicates_full_vertex() {
    try_insert_duplicates(DagTestRunner::new_full_vertex_runner())
}

#[test]
fn try_insert_duplicates_slim_vertex() {
    try_insert_duplicates(DagTestRunner::new_slim_vertex_runner())
}

fn try_insert_already_decided(mut r: DagTestRunner) {
    r.build_test_vertices([("v1_0", 14337490726892089899, vec!["genesis"])]);

    // Build two vertices for the same block
    let orig_vertex = Arc::new(r.vertices.get("v1_0").unwrap().clone());
    let block = orig_vertex.block.clone().unwrap();
    let alt_vertex = Arc::new(Vertex::new_slim(block.hash(), vec![VertexHash::default()]));

    // Should succeed to insert the first time
    assert_matches!(r.try_insert_vertices(["v1_0"]), Ok(()));
    assert!(r.frontier_matches(["v1_0"]));

    // Should return Error::DuplicateInsertion when reinserting the same vertex
    assert_matches!(
        r.dag
            .try_insert_vertices(once(orig_vertex.clone()), None, false),
        Err(avalanche::Error::DuplicateInsertion)
    );

    // Force a decision for the given vertex, causing it to become decided
    r.dag.force_decision(orig_vertex.hash(), true).unwrap();

    // Should return Error::AlreadyDecidedBlock() if inserting a new vertex for the same block
    assert_matches!(
        r.dag.try_insert_vertices(once(alt_vertex), None, false),
        Err(avalanche::Error::AlreadyDecidedBlock(_))
    );
}

#[test]
fn try_insert_already_decided_full_vertex() {
    try_insert_already_decided(DagTestRunner::new_full_vertex_runner())
}

#[test]
fn try_insert_already_decided_slim_vertex() {
    try_insert_already_decided(DagTestRunner::new_slim_vertex_runner())
}

fn try_insert_decreasing_time(mut r: DagTestRunner) {
    r.build_test_vertices([
        ("v1_0", 14337490726892089899, vec!["genesis"]),
        ("v1_1", 16012302412638312007, vec!["genesis"]),
        ("v2_0", 06720339079374117241, vec!["v1_0"]),
        ("v3_0", 12563117955961592819, vec!["v2_0"]),
        ("v3_1", 09660934870233600764, vec!["v2_0", "v1_1"]),
        ("v3_2", 14749812702066963736, vec!["v1_0", "v1_1"]),
        ("v4_0", 14143367919018540815, vec!["v3_0", "v3_1", "v3_2"]),
    ]);
    let parent = r.vertices.get("v4_0").unwrap();
    let mut block = r.build_test_block(once(Arc::new(parent.clone())), 2299226933476400337);
    block.time = parent.block.as_ref().unwrap().time - Duration::seconds(1);
    assert!(
        !r.mine_test_block(&mut block),
        "block nonce should have been {}",
        block.nonce
    );
    // Insert every test vertex. Should succeed.
    assert_matches!(
        r.try_insert_vertices(["v1_0", "v1_1", "v2_0", "v3_0", "v3_1", "v3_2", "v4_0"]),
        Ok(())
    );
    assert!(r.frontier_matches(["v4_0"]));

    // Attempt to insert the block with decreasing time. Should fail.
    assert_matches!(
        r.dag.try_insert_block(Arc::new(block.clone()), false),
        Err(avalanche::Error::Block(block::Error::DecreasingTime))
    );
}

#[test]
fn try_insert_decreasing_time_full_vertex() {
    try_insert_decreasing_time(DagTestRunner::new_full_vertex_runner())
}

#[test]
fn try_insert_decreasing_time_slim_vertex() {
    try_insert_decreasing_time(DagTestRunner::new_slim_vertex_runner())
}

// TODO: test conflicts & preference
// TODO: test with too-deep parent
// TODO: test block finalization
// TODO: test with slim vertices

// #[test]
// fn clear_pending_query() {
//     todo!();
// }
//
// #[test]
// fn get_block() {
//     todo!();
// }
//
// #[test]
// fn get_vertex() {
//     todo!();
// }
//
// #[test]
// fn handle_avalanche_message() {
//     todo!();
// }
//
// #[test]
// fn get_frontier() {
//     todo!();
// }
//
// #[test]
// fn get_frontier_hashes() {
//     todo!();
// }
//
// #[test]
// fn waitlist_processed() {
//     todo!();
// }
//
// #[test]
// fn build_undecided_vertex() {
//     todo!();
// }
//
// #[test]
// fn force_decision() {
//     todo!();
// }
//
