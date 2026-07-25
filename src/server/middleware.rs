//! Middleware layer — split from server/mod.rs (Module C2 — v2.0).

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use uuid::Uuid;

use crate::server::handler::RequestId;

pub async fn request_id_middleware(
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let id = Uuid::new_v4().to_string();
    request.extensions_mut().insert(RequestId(id.clone()));

    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::HeaderName::from_static("x-gateway-request-id"),
        header::HeaderValue::from_str(&id).unwrap(),
    );
    Ok(response)
}

pub async fn ip_guard(
    request: Request<Body>,
    next: Next,
    allowed_ips: Vec<String>,
) -> Result<Response, Response> {
    let pass = request
        .headers()
        .get("x-real-ip")
        .or_else(|| request.headers().get("x-forwarded-for"))
        .and_then(|v| v.to_str().ok())
        .map(|ip| allowed_ips.iter().any(|a| a == ip))
        .unwrap_or(true);

    if pass {
        Ok(next.run(request).await)
    } else {
        Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap())
    }
}
