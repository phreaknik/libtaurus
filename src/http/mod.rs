use hyper;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use std::result;
use std::sync::{Arc, Mutex};
use tracing::{error, info};
///
/// Error type for cordelia-p2p errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Hyper(#[from] hyper::Error),
}

/// Result type for cordelia-p2p
pub type HttpResult<T> = result::Result<T, Error>;

#[derive(Debug, Clone)]
struct ServerState {}

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

pub async fn run() -> HttpResult<()> {
    info!("Starting http service...");

    let state = Arc::new(Mutex::new(ServerState {}));
    let make_service = make_service_fn(move |_conn| {
        let state = Arc::clone(&state);
        async { Ok::<_, hyper::Error>(service_fn(move |req| handle(req, Arc::clone(&state)))) }
    });
    let server = Server::bind(&([127, 0, 0, 1], 12021).into()).serve(make_service);

    // And run forever...
    server.await.map_err(Error::from)
}
