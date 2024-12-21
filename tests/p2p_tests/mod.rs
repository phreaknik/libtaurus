//use etcetera::{app_strategy::Xdg, choose_app_strategy, AppStrategy, AppStrategyArgs};
//use std::{fs, io::ErrorKind};

//TODO: implement and enable all these tests

//fn reset_test_dirs(test_name: &str) -> Xdg {
//    let app_dirs = choose_app_strategy(AppStrategyArgs {
//        top_level_domain: "org".to_string(),
//        author: "Cordelia Devs".to_string(),
//        app_name: format!("Cordelia P2P Test {test_name}"),
//    })
//    .unwrap();
//    match fs::remove_dir_all(app_dirs.datadir().as_path()) {
//        Err(e) => {
//            if let ErrorKind::NotFound = e.kind() {
//                Ok(())
//            } else {
//                Err(e)
//            }
//        }
//        _ => Ok(()),
//    }
//    .expect("failed to remove old test data directory");
//    fs::create_dir_all(app_dirs.datadir().as_path())
//        .expect("failed to create test data directory");
//    app_dirs
//}
//
//#[tokio::test]
//async fn new() {
//    // Setup test
//    let test_dirs = reset_test_dirs("new");
//
//    // Define some test bootpeer addrs
//    let boot_peers = vec![
//        "/ip4/127.0.0.1".parse().unwrap(),
//        "/ip4/11.12.13.14/udp/37953/quic-v1/p2p/
// 12D3KooWJ4CkDESz7bfG2XLoKWwTjVUh1XftYtZh3dzo4AaD9qvG".parse().unwrap(),    ];
//
//    // Create behaviour
//    let peer_db = PeerDatabase::open(&test_dirs.datadir(), true).unwrap();
//    let cfg = behaviour::Config::new(Keypair::generate_ed25519(), boot_peers);
//    let mut _bhvr = Behaviour::new(cfg, peer_db).unwrap();
//
//    // Read peers from database, and confirm they match our boot peers
//    todo!("Impl peer_db read/write, and do readback test here");
//}
//
//#[tokio::test]
//async fn publish() {
//    todo!();
//}
//
//#[tokio::test]
//async fn report_message_validation_result() {
//    todo!();
//}
//
//#[tokio::test]
//async fn add_boot_nodes() {
//    // Setup test
//    let test_dirs = reset_test_dirs("add_boot_peers");
//
//    // Create behaviour WITHOUT BOOT PEERS
//    let peer_db = PeerDatabase::open(&test_dirs.datadir(), true).unwrap();
//    let cfg = behaviour::Config::new(Keypair::generate_ed25519(), Vec::new());
//    let mut bhvr = Behaviour::new(cfg, peer_db).unwrap();
//
//    // Manually add boot nodes
//    let boot_peers = vec![
//        "/ip4/127.0.0.1".parse().unwrap(),
//        "/ip4/11.12.13.14/udp/37953/quic-v1/p2p/
// 12D3KooWJ4CkDESz7bfG2XLoKWwTjVUh1XftYtZh3dzo4AaD9qvG".parse().unwrap(),    ];
//    let cfg = behaviour::Config::new(Keypair::generate_ed25519(), boot_peers);
//    bhvr.add_boot_nodes(&cfg);
//
//    // Read peers from database, and confirm they match our boot peers
//    todo!("Impl peer_db read/write, and do readback test here");
//}
//
//#[tokio::test]
//async fn handle_unreachable_peer() {
//    todo!();
//}
//
//#[tokio::test]
//async fn block_peer() {
//    todo!();
//}
//
//#[tokio::test]
//async fn avalanche_request() {
//    todo!();
//}
//
//#[tokio::test]
//async fn avalanche_response() {
//    todo!();
//}
//
//#[tokio::test]
//async fn handle_identify_event() {
//    todo!();
//}
//
//#[tokio::test]
//async fn bootstrap() {
//    todo!();
//}
