mod admin;
mod health;

use std::sync::Arc;

use axum::Router;

use crate::services::TreeOperations;

pub fn router(tree_operations: Arc<dyn TreeOperations>) -> Router {
    Router::new()
        .merge(health::router())
        .nest("/admin", admin::router(tree_operations))
}
