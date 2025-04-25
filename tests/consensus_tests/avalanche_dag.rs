use chrono::{DateTime, Duration};
use cordelia::{
    avalanche,
    consensus::{block, Dag},
    params,
    randomx::RandomXVMInstance,
    Block, GenesisConfig, Vertex, VertexHash, WireFormat,
};
use libp2p::{multihash::Multihash, PeerId};
use rand::{thread_rng, Rng};
use randomx_rs::RandomXFlag;
use std::{assert_matches::assert_matches, collections::HashMap, fs, path::PathBuf, sync::Arc};
use tokio::sync::{broadcast, mpsc};

/// Creates a new empty DAG with a fresh database in a temporary directory
fn setup_test_dag() -> (Dag, RandomXVMInstance) {
    let db_path: PathBuf = "test_outputs/consensus_db".into();
    let _ = fs::remove_dir_all(db_path.as_path());
    let config = avalanche::Config {
        data_dir: db_path,
        genesis: GenesisConfig {
            difficulty: params::MIN_DIFFICULTY,
            time: DateTime::parse_from_rfc2822("Wed, 18 Feb 2015 23:16:09 GMT")
                .unwrap()
                .into(),
        }
        .to_vertex(),
        waitlist_cap: 10.try_into().unwrap(),
    };
    let randomx_vm =
        RandomXVMInstance::new(b"test-key", RandomXFlag::get_recommended_flags()).unwrap();
    let (action_sender, _action_receiver) = mpsc::unbounded_channel();
    let (event_sender, _) = broadcast::channel(10);
    (
        Dag::new(config, randomx_vm.clone(), action_sender, event_sender),
        randomx_vm,
    )
}

/// Finds a nonce to satisfy the POW. Returns false if the block already had a valid nonce.
fn mine_test_block(id: &str, block: &mut Block, rxvm: &RandomXVMInstance) -> bool {
    if block.verify_pow(rxvm).is_ok() {
        false
    } else {
        block.nonce = thread_rng().gen();
        while block.verify_pow(rxvm).is_err() {
            block.nonce += 1;
        }
        println!("Block {id} should have had nonce {}", block.nonce);
        true
    }
}

/// Builds a block descending from the given parents
fn build_test_block(parents: Vec<Arc<Vertex>>, nonce: u64) -> Block {
    Block {
        version: 0,
        difficulty: params::MIN_DIFFICULTY,
        miner: PeerId::from_multihash(Multihash::default()).unwrap(),
        parents: parents.iter().map(|v| v.hash()).collect(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        time: parents
            .iter()
            .map(|v| v.block.as_ref().unwrap().time)
            .max()
            .unwrap()
            + Duration::seconds(1),
        nonce,
    }
}

/// Builds a list of vertices to create a DAG scenario.
/// Input a frontier of vertices to be built on, and a list of tuples describing the vertices to
/// build. Each tuple contains three elements:
/// 1) unique ID
/// 2) start nonce to optimize mining
/// 3) list of parent vertex IDs
fn build_test_scenario(
    rxvm: &RandomXVMInstance,
    frontier: Vec<(&str, Arc<Vertex>)>,
    descriptors: Vec<(&str, u64, Vec<&str>)>,
) -> Vec<Arc<Vertex>> {
    let mut bad_nonce = false;

    // Initialize a list of vertices with the frontier vertices
    let mut vertices: HashMap<&str, Arc<Vertex>> = frontier.clone().into_iter().collect();
    for desc in descriptors {
        let mut block = build_test_block(
            desc.2
                .iter()
                .map(|vid| {
                    vertices
                        .get(vid)
                        .expect(format!("didn't find {vid} in vertices").as_str())
                        .clone()
                })
                .collect(),
            desc.1,
        );
        bad_nonce |= mine_test_block(desc.0, &mut block, rxvm);
        vertices.insert(desc.0, Arc::new(Vertex::new_full(Arc::new(block))));
    }
    assert_eq!(bad_nonce, false, "Generated blocks have incorrect nonces");
    // Remove the frontier vertices from the scenario now
    for (vid, _) in &frontier {
        vertices.remove(vid);
    }
    vertices.into_values().collect()
}

#[test]
fn genesis_hash() {
    let (dag, _rxvm) = setup_test_dag();
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
    let mut bad_nonce = false;
    let (mut dag, rxvm) = setup_test_dag();
    let mut block = build_test_block(dag.get_frontier().unwrap(), 0);
    assert_matches!(
        dag.try_insert_block(Arc::new(block.clone()), false),
        Err(avalanche::Error::Block(block::Error::InvalidPoW))
    );
    block.nonce = 570068822920606640;
    bad_nonce |= mine_test_block("b0", &mut block, &rxvm);
    assert_matches!(dag.try_insert_block(Arc::new(block.clone()), false), Ok(()));
    block.parents.push(VertexHash::default()); // Insert missing parent
    block.nonce = 8730637501828490078;
    bad_nonce |= mine_test_block("b1", &mut block, &rxvm);
    assert_matches!(
        dag.try_insert_block(Arc::new(block), false),
        Err(avalanche::Error::Waiting)
    );
    // TODO: test broadcast decision if dag still does P2P stuff
    assert_eq!(bad_nonce, false, "Generated blocks have incorrect nonces");
}

#[test]
fn try_insert_vertices() {
    let (mut dag, rxvm) = setup_test_dag();
    let named_frontier = vec![("genesis", dag.get_frontier().unwrap()[0].clone())];
    let descriptors = vec![
        ("v1_0", 14337490726892089899, vec!["genesis"]),
        ("v1_1", 16012302412638312007, vec!["genesis"]),
        ("v2_0", 06720339079374117241, vec!["v1_0"]),
        ("v3_0", 09660934870233600764, vec!["v2_0", "v1_1"]),
    ];
    let vertices = build_test_scenario(&rxvm, named_frontier, descriptors);
    // TODO: test scenarios with waiting blocks
    // TODO: test scenarios with invalid blocks
    // TODO: test scenarios with repeated blocks
    assert_matches!(dag.try_insert_vertices(vertices, None, false), Ok(()));
    // TODO: test sender and broadcast decision if dag still does P2P stuff
}

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
