use std::{assert_matches::assert_matches, fs, path::PathBuf, sync::Arc};

use chrono::{DateTime, Utc};
use cordelia::{
    avalanche,
    consensus::{block, Dag},
    params,
    randomx::RandomXVMInstance,
    Block, GenesisConfig, VertexHash,
};
use libp2p::{multihash::Multihash, PeerId};
use randomx_rs::RandomXFlag;
use tokio::sync::{broadcast, mpsc};

fn setup_test_dag() -> Dag {
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
    Dag::new(config, randomx_vm, action_sender, event_sender)
}

#[test]
fn genesis_hash() {
    let dag = setup_test_dag();
    assert_eq!(
        dag.genesis_hash(),
        VertexHash::with_bytes([
            212, 240, 55, 179, 70, 28, 196, 11, 130, 42, 154, 243, 206, 30, 90, 74, 196, 164, 192,
            169, 68, 15, 204, 248, 53, 149, 239, 50, 79, 111, 250, 245
        ])
    );
}

#[test]
fn try_insert_block() {
    let mut dag = setup_test_dag();
    let mut block = Block {
        version: 0,
        difficulty: 0,
        miner: PeerId::from_multihash(Multihash::default()).unwrap(),
        parents: dag.get_frontier(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        time: DateTime::parse_from_rfc2822("Wed, 18 Feb 2015 23:26:09 GMT")
            .unwrap()
            .into(),
        nonce: 0,
    };
    assert_matches!(
        dag.try_insert_block(Arc::new(block.clone()), false),
        Err(avalanche::Error::Block(block::Error::InvalidDifficulty))
    );
    block.difficulty = params::MIN_DIFFICULTY;
    assert_matches!(
        dag.try_insert_block(Arc::new(block.clone()), false),
        Err(avalanche::Error::Block(block::Error::InvalidPoW))
    );
    //while let Err(avalanche::Error::Block(block::Error::InvalidPoW)) =
    //    dag.try_insert_block(Arc::new(block.clone()), false)
    //{
    //    block.nonce += 1;
    //}
    //println!("NONCE = {}", block.nonce);
    block.nonce = 743;
    assert_matches!(dag.try_insert_block(Arc::new(block), false), Ok(()));
    // TODO: test broadcast decision
}

// #[test]
// fn try_insert_vertices() {
//     todo!();
// }
//
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
// fn waitlist_processed() {
//     todo!();
// }
