use super::handler;
use clap::ArgMatches;
use etcetera::{base_strategy::choose_native_strategy, BaseStrategy};
use libp2p::{identity::Keypair, kad};
use libtaurus::{
    consensus::{self, dag, GenesisConfig},
    p2p, rpc, WireFormat,
};
use rand::{distributions::Alphanumeric, Rng};
use std::{char, env, fs, path::PathBuf};

/// Build app data directory
pub fn app_data_dir(args: &ArgMatches) -> PathBuf {
    args.get_one("datadir").cloned().unwrap_or_else(|| {
        choose_native_strategy()
            .map(|strat| {
                if args.get_flag("tmpdir") {
                    env::temp_dir().join("taurus/").join(
                        rand::thread_rng()
                            .sample_iter(&Alphanumeric)
                            .take(8)
                            .map(char::from)
                            .collect::<String>(),
                    )
                } else {
                    strat.data_dir().join("taurus/")
                }
            })
            .unwrap()
    })
}

/// Build P2P [`p2p::Config`] from parsed CLI args
pub fn build_p2p_cfg(args: &ArgMatches) -> p2p::task::Config {
    let datadir = app_data_dir(args).join("p2p/");
    let identity_key = get_peer_identity_key(&datadir);
    p2p::task::Config {
        datadir,
        identity_key,
        boot_peers: args
            .get_many("bootpeer")
            .map(|p| p.cloned().collect())
            .unwrap_or(Vec::new()),
        addr: *args.get_one("bind").unwrap(),
        port: *args.get_one("port").unwrap(),
        search_port: args.get_flag("portsearch"),
        kad_mode_override: args.get_one::<String>("dhtmode").map(|s| match s.as_str() {
            "client" => kad::Mode::Client,
            "server" => kad::Mode::Server,
            _ => unreachable!(),
        }),
    }
}

/// Build consensus [`consensus::Config`] from parsed CLI args
pub fn build_consensus_cfg(args: &ArgMatches) -> consensus::task::Config {
    let genesis_cfg = GenesisConfig {};
    let genesis_hash = genesis_cfg.to_vertex().hash();
    consensus::task::Config {
        genesis: genesis_cfg,
        datadir: app_data_dir(args).join("consensus/"),
        dag: dag::Config {
            genesis: genesis_hash,
            ..dag::Config::default()
        },
        ..consensus::task::Config::default()
    }
}

/// Build handler [`handler::Config`] from parsed CLI args
pub fn build_handler_cfg(_args: &ArgMatches) -> handler::Config {
    handler::Config {
        ..handler::Config::default()
    }
}

/// Build RPC [`rpc::Config`] from parsed CLI args
pub fn build_rpc_cfg(args: &ArgMatches) -> rpc::task::Config {
    rpc::task::Config {
        bind_addr: *args.get_one("rpcbind").unwrap(),
        bind_port: *args.get_one("rpcport").unwrap(),
        search_port: args.get_flag("rpcportsearch"),
    }
}

pub fn get_peer_identity_key(datadir: &PathBuf) -> Keypair {
    let keypath = datadir.join("identity_key");
    let _ = fs::create_dir_all(&datadir);
    match fs::read(&keypath) {
        Ok(keydata) => {
            Keypair::from_protobuf_encoding(&keydata).expect("Failed to decode keyfile!")
        }
        Err(_) => {
            // Generate a random new key
            let newkey = Keypair::generate_ed25519();
            // Save the key to the file
            fs::write(
                keypath,
                newkey
                    .to_protobuf_encoding()
                    .expect("Failed to encode key to save to keyfile!"),
            )
            .expect("Failed to write to keyfile!");
            newkey
        }
    }
}
