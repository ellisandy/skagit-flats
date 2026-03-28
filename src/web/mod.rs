use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;

use crate::app::SharedState;

/// Build the axum Router for the local web interface.
///
/// Endpoints:
/// - GET /health   — 200 OK (liveness check)
/// - GET /preview  — current PixelBuffer rendered as PNG (image/png)
/// - GET /sources  — JSON list of sources with status
pub fn build_router(state: Arc<SharedState>) -> Router {
    Router::new()
        .route("/health", get(handler_health))
        .route("/preview", get(handler_preview))
        .route("/sources", get(handler_sources))
        .with_state(state)
}

async fn handler_health() -> &'static str {
    "OK"
}

async fn handler_preview(State(state): State<Arc<SharedState>>) -> impl IntoResponse {
    let buf = state.pixel_buffer.read().expect("pixel_buffer lock poisoned");
    let png_bytes = buf.to_png();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/png")],
        png_bytes,
    )
}

async fn handler_sources(State(state): State<Arc<SharedState>>) -> impl IntoResponse {
    let statuses = state.source_statuses.read().expect("source_statuses lock poisoned");
    let json = serde_json::to_string(&*statuses).unwrap_or_else(|_| "[]".to_string());
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        json,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use crate::app::{SharedState, SourceStatus};
    use crate::render::PixelBuffer;
    use std::sync::RwLock;
    use tower::ServiceExt;

    fn test_state() -> Arc<SharedState> {
        Arc::new(SharedState {
            pixel_buffer: RwLock::new(PixelBuffer::new(800, 480)),
            source_statuses: RwLock::new(vec![SourceStatus {
                name: "weather".to_string(),
                enabled: true,
                last_fetch: Some(1000),
                last_error: None,
                next_fetch: Some(1300),
            }]),
        })
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn preview_returns_png() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/preview").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "image/png"
        );
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        // PNG magic bytes
        assert_eq!(&body[..4], &[0x89, b'P', b'N', b'G']);
    }

    #[tokio::test]
    async fn sources_returns_json() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/sources").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/json"
        );
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["name"], "weather");
    }
}
