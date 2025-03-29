use tokio;

#[tokio::main]
async fn main() {
    // Start the P2P client
    cordelia_p2p::run(std::env::args().nth(1)).await.unwrap();
}
