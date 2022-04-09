#[macro_use]
extern crate tracing;

#[macro_use]
extern crate lazy_static;

use axum::{
    body::{Body, Bytes},
    response::IntoResponse,
    routing::{any, get},
    Error, Router,
};
use http::{Request, Response};
use http_body::combinators::UnsyncBoxBody;
use tokio::sync::Mutex;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const EXPOSE_INTERNAL_ERRORS: bool = false;
const BEHIND_PROXY: bool = false;
const ALLOW_DOTENV_ON_DEBUG: bool = true;

lazy_static! {
    static ref COUNTER: Mutex<u8> = Mutex::new(0);
}

async fn count() -> Response<UnsyncBoxBody<Bytes, Error>> {
    (http::StatusCode::OK, format!("{}", *COUNTER.lock().await)).into_response()
}

async fn reset() {
    *COUNTER.lock().await = 0;
}

async fn handler(req: Request<Body>) -> Response<Body> {
    let mut counter = COUNTER.lock().await;
    *counter += 1;
    drop(counter);

    cf_route_services::proxy_request::<EXPOSE_INTERNAL_ERRORS>(req).await
}

#[tokio::main]
async fn main() {
    // Used to see all tracing logs of level info in stdout
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Sets up an api `Router` to get and delete the count
    let api_router = Router::new().route("/api/count", get(count).delete(reset));

    // Proxy setup to handle any request received
    let proxy_router = Router::new().route("/", any(handler));

    cf_route_services::serve::<BEHIND_PROXY, ALLOW_DOTENV_ON_DEBUG>(Some(api_router), Some(proxy_router)).await;
    
    info!("shut down, self cleanup");
}
