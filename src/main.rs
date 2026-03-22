use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use local_ip_address::local_ip;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{net::SocketAddr, sync::Arc, time::Instant};
use tracing::info;
use url::Url;
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

#[derive(Clone)]
struct AppState {
    started_at: Instant,
    client: reqwest::Client,
}

#[derive(Debug, Serialize, ToSchema)]
struct ApiResponse<T: Serialize> {
    ok: bool,
    data: T,
}

#[derive(Debug, Serialize, ToSchema)]
struct ErrorResponse {
    ok: bool,
    error: String,
}

#[derive(Debug, Serialize, ToSchema)]
struct HelloData {
    message: String,
    service: String,
    version: String,
}

#[derive(Debug, Serialize, ToSchema)]
struct HealthData {
    status: String,
    uptime_seconds: u64,
    ip_address: String,
    hostname: String,
    process_id: u32,
    rust_env: String,
    service: String,
    version: String,
}

#[derive(Debug, Deserialize, IntoParams)]
struct ProxyParams {
    /// Absolute http(s) URL returning JSON.
    url: String,
}

#[derive(Debug, Serialize, ToSchema)]
struct ProxyData {
    proxied_url: String,
    status: u16,
    content_type: Option<String>,
    body: Value,
}

#[derive(OpenApi)]
#[openapi(
    paths(root_handler, healthz_handler, proxy_handler, openapi_handler),
    components(
        schemas(
            ApiResponse<HelloData>,
            ApiResponse<HealthData>,
            ApiResponse<ProxyData>,
            ErrorResponse,
            HelloData,
            HealthData,
            ProxyData
        )
    ),
    tags(
        (name = "axum-demo", description = "Ready-to-use Axum JSON demo API")
    ),
    info(
        title = "axum-demo API",
        version = "0.1.0",
        description = "Demo Axum service with JSON responses, health checks, proxying, and generated OpenAPI docs."
    )
)]
struct ApiDoc;

use utoipa::IntoParams;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "axum_demo=info,tower_http=info".into()),
        )
        .init();

    let state = Arc::new(AppState {
        started_at: Instant::now(),
        client: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .expect("failed to build reqwest client"),
    });

    let app = build_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    info!(%addr, "starting axum-demo");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind tcp listener");

    axum::serve(listener, app)
        .await
        .expect("server error");
}

fn build_router(state: Arc<AppState>) -> Router {
    let openapi = ApiDoc::openapi();

    Router::new()
        .route("/", get(root_handler))
        .route("/healthz", get(healthz_handler))
        .route("/proxy", get(proxy_handler))
        .merge(SwaggerUi::new("/docs").url("/openapi.json", openapi))
        .with_state(state)
}

#[utoipa::path(
    get,
    path = "/",
    tag = "axum-demo",
    responses(
        (status = 200, description = "Hello endpoint", body = ApiResponse<HelloData>)
    )
)]
async fn root_handler() -> impl IntoResponse {
    Json(ApiResponse {
        ok: true,
        data: HelloData {
            message: "hello from axum-demo".to_string(),
            service: "axum-demo".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
    })
}

#[utoipa::path(
    get,
    path = "/healthz",
    tag = "axum-demo",
    responses(
        (status = 200, description = "Server health and runtime status", body = ApiResponse<HealthData>)
    )
)]
async fn healthz_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let uptime_seconds = state.started_at.elapsed().as_secs();
    let ip_address = local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let hostname = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());

    Json(ApiResponse {
        ok: true,
        data: HealthData {
            status: "ok".to_string(),
            uptime_seconds,
            ip_address,
            hostname,
            process_id: std::process::id(),
            rust_env: std::env::var("RUST_ENV").unwrap_or_else(|_| "development".to_string()),
            service: "axum-demo".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
    })
}

#[utoipa::path(
    get,
    path = "/proxy",
    tag = "axum-demo",
    params(ProxyParams),
    responses(
        (status = 200, description = "Proxied upstream JSON payload", body = ApiResponse<ProxyData>),
        (status = 400, description = "Invalid proxy target", body = ErrorResponse),
        (status = 502, description = "Upstream request or JSON parsing failed", body = ErrorResponse)
    )
)]
async fn proxy_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ProxyParams>,
) -> impl IntoResponse {
    match validate_proxy_url(&params.url) {
        Ok(url) => match fetch_json(&state.client, url).await {
            Ok(proxy_data) => (StatusCode::OK, Json(ApiResponse { ok: true, data: proxy_data })).into_response(),
            Err((status, message)) => error_response(status, &message),
        },
        Err(message) => error_response(StatusCode::BAD_REQUEST, &message),
    }
}

fn validate_proxy_url(input: &str) -> Result<Url, String> {
    let url = Url::parse(input).map_err(|e| format!("invalid url: {e}"))?;

    match url.scheme() {
        "http" | "https" => {}
        _ => return Err("only http and https urls are allowed".to_string()),
    }

    if let Some(host) = url.host_str() {
        let blocked = ["localhost", "127.0.0.1", "0.0.0.0", "::1"];
        if blocked.contains(&host) {
            return Err("proxying to localhost-style addresses is not allowed".to_string());
        }
    }

    Ok(url)
}

async fn fetch_json(client: &reqwest::Client, url: Url) -> Result<ProxyData, (StatusCode, String)> {
    let response = client
        .get(url.clone())
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("proxy request failed: {e}")))?;

    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let body = response
        .json::<Value>()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("upstream did not return valid json: {e}")))?;

    Ok(ProxyData {
        proxied_url: url.to_string(),
        status: status.as_u16(),
        content_type,
        body,
    })
}

fn error_response(status: StatusCode, message: &str) -> axum::response::Response {
    (
        status,
        Json(ErrorResponse {
            ok: false,
            error: message.to_string(),
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, routing::get, Json, Router};
    use http_body_util::BodyExt;
    use tower::util::ServiceExt;

    fn test_state() -> Arc<AppState> {
        Arc::new(AppState {
            started_at: Instant::now(),
            client: reqwest::Client::builder().build().unwrap(),
        })
    }

    #[tokio::test]
    async fn root_returns_json() {
        let app = build_router(test_state());

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(body["ok"], true);
        assert_eq!(body["data"]["message"], "hello from axum-demo");
    }

    #[tokio::test]
    async fn healthz_returns_json() {
        let app = build_router(test_state());

        let response = app
            .oneshot(Request::builder().uri("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(body["ok"], true);
        assert_eq!(body["data"]["status"], "ok");
        assert!(body["data"]["uptime_seconds"].is_number());
    }

    #[tokio::test]
    async fn proxy_rejects_localhost_targets() {
        let app = build_router(test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/proxy?url=http://127.0.0.1:8080/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["ok"], false);
    }

    #[tokio::test]
    async fn openapi_json_is_available() {
        let app = build_router(test_state());

        let response = app
            .oneshot(Request::builder().uri("/openapi.json").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(body["openapi"], "3.1.0");
        assert!(body["paths"]["/"].is_object());
        assert!(body["paths"]["/healthz"].is_object());
        assert!(body["paths"]["/proxy"].is_object());
    }

    #[tokio::test]
    async fn fetch_json_helper_parses_upstream_json() {
        async fn upstream() -> Json<Value> {
            Json(serde_json::json!({"hello": "world"}))
        }

        let upstream_app = Router::new().route("/demo", get(upstream));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, upstream_app).await.unwrap();
        });

        let client = reqwest::Client::builder().build().unwrap();
        let url = Url::parse(&format!("http://{}:{}/demo", addr.ip(), addr.port())).unwrap();
        let proxied = fetch_json(&client, url).await.unwrap();

        assert_eq!(proxied.status, 200);
        assert_eq!(proxied.body["hello"], "world");
    }
}
