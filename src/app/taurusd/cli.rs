use clap::{arg, command, ArgMatches};
use libp2p::Multiaddr;
use std::{env, net::Ipv4Addr, path::PathBuf};

/// Parse CLI args
pub fn parse_cli_args() -> ArgMatches {
    command!() // initialize CLI with details from cargo.toml
        .about("Start taurusd network client")
        .arg(
            arg!(--bind <ADDR> "Address to bind for P2P connections")
                .required(false)
                .default_value("0.0.0.0")
                .value_parser(clap::value_parser!(Ipv4Addr)),
        )
        .arg(
            arg!(--bootpeer <MULTIADDR> "Specify a boot peer to connect to")
                .required(false)
                .value_parser(clap::value_parser!(Multiaddr)),
        )
        .arg(
            arg!(-d --datadir <PATH> "Specify data directory")
                .required(false)
                .conflicts_with("tmpdir")
                .value_parser(clap::value_parser!(PathBuf)),
        )
        .arg(
            arg!(--dhtmode <MODE> "Set Kademlia DHT mode")
                .required(false)
                .value_parser(["client", "server"]),
        )
        .arg(
            arg!(--loglevel <LEVEL> "Set log level")
                .required(false)
                .default_value("info")
                .value_parser(["info", "debug", "trace"]),
        )
        .arg(
            arg!(--port <PORT> "Port number to accept P2P connections")
                .required(false)
                .default_value("9047")
                .value_parser(clap::value_parser!(u16)),
        )
        .arg(
            arg!(--portsearch "Increment P2P port number until an available port is found")
                .required(false),
        )
        .arg(
            arg!(--rpcbind <ADDR> "Address to bind for JSON RPC")
                .required(false)
                .default_value("127.0.0.1")
                .value_parser(clap::value_parser!(Ipv4Addr)),
        )
        .arg(
            arg!(--rpcport <PORT> "Port number to accept JSON RPC http/ws connections")
                .required(false)
                .default_value("9048")
                .value_parser(clap::value_parser!(u16)),
        )
        .arg(
            arg!(--rpcportsearch "Increment RPC port number until an available port is found")
                .required(false),
        )
        .arg(
            arg!(--tmpdir "Use a temporary data directory. Useful for testing.")
                .conflicts_with("datadir")
                .required(false),
        )
        .get_matches()
}
