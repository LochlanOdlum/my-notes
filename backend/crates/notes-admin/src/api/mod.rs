mod admin;
mod health;

use std::sync::Arc;

use axum::{
    Router,
    http::{HeaderValue, Method, header},
};
use tower_http::cors::CorsLayer;

use crate::services::TreeOperations;

pub fn router(tree_operations: Arc<dyn TreeOperations>) -> Router {
    Router::new()
        .merge(health::router())
        .nest("/admin", admin::router(tree_operations))
        .layer(
            CorsLayer::new()
                .allow_origin(HeaderValue::from_static("http://localhost:5173"))
                .allow_methods([Method::GET, Method::POST, Method::PUT, Method::OPTIONS])
                .allow_headers([
                    header::AUTHORIZATION,
                    header::CONTENT_TYPE,
                    header::IF_MATCH,
                ])
                .expose_headers([header::ETAG]),
        )
}
