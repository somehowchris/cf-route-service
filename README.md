# cf-route-services

Helps you write your [Cloud Foundry Route Services](https://docs.cloudfoundry.org/services/route-services.html) with ease using [axum](https://github.com/tokio-rs/axum).

## Why

Writing Route Service Apps for Cloud Foundry is awesome but its not boilerplate free.

 - Manage headers on proxyied  requests and responses
 - Differentiate between proxy and api requests

So what does this crate do?

 - Manage `X-CF-PROXY*` headers on requests and responses
 - Manages 2 different routers, one for api and one for proxy requests
 - Manages `X-Request-Id` to be end-to-end
 - Integrates with `tracing` to give you debug, error and info messages as well as spans to differentiate api and proxy requests
 - Exposes a function to proxy a given request
 - Manages graceful shutdown
 - It will run on the port specified by cf on releases and lets you use `dotenv` on debug builds

This allows you to write your app as a normal axum app without needing to think of the needless `What if`s.

## How to get started

Let's add the crate to your `Cargo.toml`:
```sh
cargo add cf-route-services
```

After that, the simplest you can start with is the following:
```rust
const BEHIND_PROXY: bool = false;
const ALLOW_DOTENV_ON_DEBUG: bool = false;

#[tokio::main]
async fn main() {
    cf_route_services::serve::<BEHIND_PROXY, ALLOW_DOTENV_ON_DEBUG>(None, None).await;
}
```
> This will setup a route service with no api and a proxy to proxy every request as is.

An a bit more real world example would be counting requests.

```rust
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
const ALLOW_DOTENV_ON_DEBUG: bool = false;

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

```



## TODO

- CICD
- Unit Tests
- Validate function in cf
- Write in code docs
- Write readmem