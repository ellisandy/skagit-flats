use axum::{routing::get, Router};

/// Build the axum Router for the local web interface.
///
/// Endpoints:
/// - GET /preview  — current PixelBuffer as PNG (pixel-identical to the display)
/// - GET /sources  — list all sources with status
/// - GET /destinations — list configured destinations and their TripDecision
/// - POST /destinations — create or update a destination
/// - DELETE /destinations/:name — remove a destination
/// - POST /sources/:name/enable
/// - POST /sources/:name/disable
///
/// This is a stub for Wave 1. Full implementation is in a later wave.
pub fn build_router() -> Router {
    Router::new()
        .route("/preview", get(handler_preview))
        .route("/sources", get(handler_sources))
        .route("/destinations", get(handler_destinations))
}

async fn handler_preview() -> &'static str {
    "preview stub"
}

async fn handler_sources() -> &'static str {
    "sources stub"
}

async fn handler_destinations() -> &'static str {
    "destinations stub"
}
