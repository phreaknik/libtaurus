use cordelia_p2p::Config;
use futures::prelude::*;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::{identity, Multiaddr, PeerId};
use std::net::Ipv4Addr;
use tokio;

#[tokio::main]
async fn main() {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {local_peer_id:?}");

    // Build the swarm
    // TODO: pick a different transport. development_transport() has features we likely don't want,
    // e.g. noise encryption
    let mut swarm = SwarmBuilder::with_tokio_executor(
        libp2p::development_transport(local_key.clone())
            .await
            .expect("failed to construct transport"),
        cordelia_p2p::Behaviour::new(Config::new(local_key.public())),
        local_peer_id,
    )
    .build();

    // Listen for inbound connections
    swarm
        .listen_on(
            Multiaddr::empty()
                .with(Protocol::Ip4(Ipv4Addr::UNSPECIFIED))
                .with(Protocol::Tcp(0)),
        )
        .expect("failed to build swarm");

    // Dial the peer identified by the multi-address given as the second
    // command-line argument, if any.
    if let Some(addr) = std::env::args().nth(1) {
        let remote: Multiaddr = addr.parse().expect("bad address");
        if swarm.dial(remote).is_err() {
            println!("error: failed to dial peer");
        } else {
            println!("Dialed {addr}")
        }
    }

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => println!("Listening on {address:?}"),
            SwarmEvent::Behaviour(event) => println!("{event:?}"),
            _ => {}
        }
    }
}
