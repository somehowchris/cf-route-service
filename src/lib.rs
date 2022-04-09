#![forbid(unsafe_code)]
#![deny(clippy::all)]

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate tracing;

pub mod headers;

use axum::routing::any;
use axum::{
    body::Body,
    extract::{FromRequest, RequestParts},
    response::{IntoResponse, Response},
    Router,
};
use axum_client_ip::ClientIp;
use http::{HeaderValue, Request, StatusCode};
use std::net::SocketAddr;
use tokio::signal;
use tower::Service;
use tower_http::{
    compression::CompressionLayer,
    request_id::{MakeRequestId, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer},
    LatencyUnit,
};
use tracing::Level;
use uuid::Uuid;

#[derive(Clone, Copy, Default)]
struct XRequestId;

impl MakeRequestId for XRequestId {
    fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
        Some(RequestId::new(
            HeaderValue::from_str(&Uuid::new_v4().to_string()).unwrap(),
        ))
    }
}

pub async fn serve<const BEHIND_PROXY: bool, const ALLOW_DOTENV_ON_DEBUG: bool>(
    api_router: Option<Router>,
    proxy_router: Option<Router>,
) {
    if ALLOW_DOTENV_ON_DEBUG {
        #[cfg(debug_assertions)]
        dotenvy::dotenv().ok();

        #[cfg(debug_assertions)]
        info!("Using .env file if present");
    }
    
    let proxy_router = proxy_router.unwrap_or_else(|| {
        Router::new().route(
            "/",
            any(|req: Request<Body>| async { proxy_request::<false>(req).await }),
        )
    });

    let api_router = api_router.unwrap_or_else(Router::new);

    if let Ok(port) = cf_env::get_port() {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));

        let service = tower::service_fn(move |req: Request<_>| {
            let mut api = api_router.clone();
            let mut proxy = proxy_router.clone();

            async move {
                debug!("checking for any cf route service headers");

                if headers::http::ROUTE_SERVICES_HEADERS_LIST
                    .iter()
                    .any(|header| req.headers().contains_key(*header))
                {
                    for header in *headers::http::ROUTE_SERVICES_HEADERS_LIST {
                        if !req.headers().contains_key(header)
                            || req
                                .headers()
                                .get(header)
                                .unwrap()
                                .to_str()
                                .unwrap()
                                .trim()
                                .is_empty()
                        {
                            debug!("Did not find {} header in request", header);

                            return Ok((
                                StatusCode::BAD_REQUEST,
                                format!("Did not find {} header in request", header),
                            )
                                .into_response());
                        }
                    }

                    info_span!(
                        "proxy",
                        otel.name=%format!("Proxy {} {}", req.method(), req.headers().get(&*headers::http::X_CF_FORWARDED_URL).unwrap().to_str().unwrap()),
                    ).in_scope(|| async {
                        info!("Calling proxy with {} on {}", req.method(), req.headers().get(&*headers::http::X_CF_FORWARDED_URL).unwrap().to_str().unwrap());
                    let response = proxy.call(req).await;

                    if let Ok(mut response) = response {
                        debug!("Removing cf route service headers on response");

                        for header in *headers::http::ROUTE_SERVICES_HEADERS_LIST {
                            if response.headers().contains_key(header) {
                                response.headers_mut().remove(header).unwrap();
                            }
                        }

                        debug!("Returning proxy response");
                        Ok(response)
                    } else {
                        debug!("Calling proxy failed");
                        response
                    }}).await
                } else {
                    info_span!(
                        "api",
                        otel.name=%format!("API {} {}", req.method(), req.uri().path()),
                    )
                    .in_scope(|| async {
                        debug!("Calling api because X-CF-FORWARDED-URL header was not found");
                        api.call(req).await
                    })
                    .await
                }
            }
        });

        info!("Running on port {}", port);
        let app = Router::new().route("/", axum::routing::any_service(service))            .layer(CompressionLayer::new())
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(TraceLayer::new_for_http().make_span_with(|request: &Request<Body>| {
            let request_id = request.headers().get(&*headers::http::X_REQUEST_ID).unwrap().to_str().unwrap();
            info!("Received request {} using {} on {}", request_id,request.method(), request.uri().path());

            tracing::info_span!("request",
                otel.name=%format!("{} {}", request.method(), request.uri().path()),
                http.method = %request.method(),
                http.uri = %request.uri().path(),
                http.user_agent=%request.headers().get("User-Agent").map(|el|el.to_str().unwrap()).unwrap_or("No user agent set"),
                http.status_code = tracing::field::Empty,
                http.request_id=%request_id
            )
        })
        .on_request(
            DefaultOnRequest::new()
                .level(Level::INFO)
        )
        .on_response(
            DefaultOnResponse::new()
                .level(Level::INFO)
                .latency_unit(LatencyUnit::Micros)
                .include_headers(true)
        ))
        .layer(SetRequestIdLayer::x_request_id(XRequestId{}));

        if BEHIND_PROXY {
            return axum::Server::bind(&addr)
                .serve(app.into_make_service())
                .with_graceful_shutdown(shutdown_signal())
                .await
                .unwrap();
        } else {
            debug!("Running behind proxy");

            return axum::Server::bind(&addr)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .with_graceful_shutdown(shutdown_signal())
                .await
                .unwrap();
        };
    }

    error!("Environment should define 'PORT' environment variable");
    std::process::exit(1);
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct InternalError {
    status: u16,
    message: String,
}

pub async fn proxy_request<const FORWARD_INTERNAL_ERRORS: bool>(
    req: Request<Body>,
) -> Response<Body> {
    info_span!("proxy request").in_scope(||async {
        let headers = req.headers().clone();
        let mut parts = RequestParts::new(req);

        match ClientIp::from_request(&mut parts).await {
            Ok(client_ip) => match headers.get(&*headers::http::X_CF_FORWARDED_URL) {
                Some(forward_url) => {
                    debug!("calling proxy");
                    match hyper_reverse_proxy::call(
                        client_ip.0,
                        forward_url.to_str().unwrap(),
                        parts.try_into_request().unwrap(),
                    )
                    .await
                    {
                        Ok(response) => response,
                        Err(value) => {
                            error!("Encountered internal error: {:?}", value);
                            if FORWARD_INTERNAL_ERRORS {
                                let error = InternalError {
                                    status: 500,
                                    message: format!("Internal Error: {:?}", value),
                                };

                                Response::builder()
                                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                                    .header(http::header::CONTENT_TYPE, "application/json")
                                    .body(Body::from(serde_json::to_string(&error).unwrap()))
                                    .unwrap()
                            } else {
                                Response::builder()
                                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                                    .body(Body::empty())
                                    .unwrap()
                            }
                        }
                    }
                }
                None => {
                    error!("Header x-cf-forwarded-for needs to be set to proxy request. Found the following: {:?}", headers);
                    Response::builder()
                        .status(400)
                        .body(Body::from(
                            "Header x-cf-forwarded-for needs to be set to proxy request",
                        ))
                        .unwrap()
                }
            },
            Err(err) => {
                error!("Failed to get client ip: {:?}", err);
                Response::builder()
                    .status(err.0)
                    .body(Body::from(err.1))
                    .unwrap()
            }
        }
    }).await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("signal received, starting graceful shutdown");
}
