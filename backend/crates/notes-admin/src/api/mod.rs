mod admin;
mod health;

use axum::Router;

pub fn router() -> Router {
    Router::new()
        .merge(health::router())
        .nest("/admin", admin::router())
}
