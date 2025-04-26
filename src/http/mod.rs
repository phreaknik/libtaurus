use crate::{consensus, p2p};
use hyper::{
    service::{make_service_fn, service_fn},
    Body, Method, Request, Response, Server, StatusCode,
};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, info};

/// Error type for cordelia-p2p errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Hyper(#[from] hyper::Error),
}

/// Configuration details for the http server.
#[derive(Debug, Clone)]
pub struct Config {}

/// Internal state of the HTTP server
#[derive(Debug, Clone)]
struct ServerState {}

/// HTTP request handler
async fn handle(
    mut req: Request<Body>,
    _state: Arc<Mutex<ServerState>>,
) -> Result<Response<Body>, hyper::Error> {
    let req_version = req.version();
    if req_version == hyper::Version::HTTP_10 {
        req.extensions_mut().insert(hyper::Version::HTTP_11);
    }
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/version") => {
            let pkg_name = env!("CARGO_PKG_NAME");
            let pkg_version = env!("CARGO_PKG_VERSION");
            let response = Response::new(Body::from(format!("{pkg_name}-{pkg_version}")));
            Ok(response)
        }
        _ => {
            let response = Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap();
            Ok(response)
        }
    }
}

/// Run the http server, spawning the task as a new thread.
pub fn start(
    config: Config,
    p2p_action_ch: UnboundedSender<p2p::Action>,
    consensus_action_ch: UnboundedSender<consensus::Action>,
) {
    tokio::spawn(task_fn(config, p2p_action_ch, consensus_action_ch));
}

/// The task function which runs the consensus process.
pub async fn task_fn(
    _config: Config,
    _p2p_action_ch: UnboundedSender<p2p::Action>,
    _consensus_action_ch: UnboundedSender<consensus::Action>,
) {
    info!("Starting http service...");

    let state = Arc::new(Mutex::new(ServerState {}));
    let make_service = make_service_fn(move |_conn| {
        let state = Arc::clone(&state);
        async { Ok::<_, hyper::Error>(service_fn(move |req| handle(req, Arc::clone(&state)))) }
    });
    let server = Server::bind(&([127, 0, 0, 1], 12021).into()).serve(make_service);

    // And run forever...
    server
        .await
        .map_err(Error::from)
        .expect("http server exited unexpectedly");
}
