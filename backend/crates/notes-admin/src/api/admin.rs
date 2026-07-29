use axum::{Router, extract::Request};
use lambda_http::RequestExt;

use crate::error::ApiError;

pub fn router() -> Router {
    Router::new().fallback(not_implemented)
}

async fn not_implemented(request: Request) -> Result<ApiError, ApiError> {
    if auth_is_enabled() && !is_admin(&request) {
        return Err(ApiError::Forbidden);
    }

    Ok(ApiError::NotImplemented)
}

fn auth_is_enabled() -> bool {
    auth_is_enabled_from(std::env::var("ADMIN_AUTH_ENABLED").ok().as_deref())
}

fn auth_is_enabled_from(value: Option<&str>) -> bool {
    value == Some("true")
}

fn is_admin(request: &Request) -> bool {
    request
        .request_context_ref()
        .and_then(|context| context.authorizer())
        .and_then(|authorizer| authorizer.jwt.as_ref())
        .and_then(|jwt| jwt.claims.get("cognito:groups"))
        .is_some_and(|groups| is_member_of_admin_group(groups))
}

fn is_member_of_admin_group(groups: &str) -> bool {
    groups
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|group| group.trim().trim_matches('"'))
        .any(|group| group == "admins")
}

#[cfg(test)]
mod tests {
    use super::{auth_is_enabled_from, is_member_of_admin_group};

    #[test]
    fn recognises_the_admins_cognito_group() {
        assert!(is_member_of_admin_group("[admins]"));
        assert!(is_member_of_admin_group("[\"editors\", \"admins\"]"));
        assert!(!is_member_of_admin_group("[editors]"));
    }

    #[test]
    fn authentication_is_disabled_unless_explicitly_enabled() {
        assert!(!auth_is_enabled_from(None));
        assert!(auth_is_enabled_from(Some("true")));
        assert!(!auth_is_enabled_from(Some("false")));
    }
}
