//! The queries cache over HTTP (`sa_core::queries_cache`), open to everyone who might run the same
//! query: bounded as ingest is, and rate-limited per client at the edge.
//!
//! ```text
//! GET    /api/queries_cache/{scheme}/{key}         → 200 Stored | 404
//! POST   /api/queries_cache/{scheme}/{key}/claim   ClaimBody → 200 Claim
//! DELETE /api/queries_cache/{scheme}/{key}/claim   → 204: the buy failed, the claim given up
//! PUT    /api/queries_cache/{scheme}/{key}         Put → 204; 400 a request the key is not the digest of
//! ```

use axum::{
	Json, Router,
	body::{Body, to_bytes},
	error_handling::HandleErrorLayer,
	extract::{OriginalUri, Path, State},
	http::{Method, StatusCode},
	response::{IntoResponse, Response},
	routing::{get, post},
};
use jiff::Timestamp;
use sa_core::queries_cache::{ClaimBody, Key, Put, Scheme};
use sa_panel::Panel;
use sha2::{Digest, Sha256};
use tower::{BoxError, ServiceBuilder};
use tower_http::timeout::{RequestBodyTimeoutLayer, TimeoutLayer};

use crate::{
	http::Limits,
	ingest::{body_error, error},
};

/// A DataForSEO expansion runs to a few MiB.
const MAX_BODY: usize = 8 * 1024 * 1024;

pub(crate) fn routes(panel: Panel, limits: &Limits) -> Router {
	let layers = ServiceBuilder::new()
		.layer(HandleErrorLayer::new(|_: BoxError| async { error(StatusCode::SERVICE_UNAVAILABLE, "busy, try again") }))
		.load_shed()
		.concurrency_limit(limits.max_concurrent)
		.layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, limits.request_timeout))
		.layer(RequestBodyTimeoutLayer::new(limits.body_timeout));
	Router::new()
		.route("/api/queries_cache/{scheme}/{key}", get(query).put(query).layer(layers.clone()))
		.route("/api/queries_cache/{scheme}/{key}/claim", post(query).delete(query).layer(layers))
		.with_state(panel)
}

/// Every route: what its method asks of the cache.
async fn query(State(panel): State<Panel>, Path((scheme, key)): Path<(String, String)>, method: Method, OriginalUri(uri): OriginalUri, body: Body) -> Response {
	let body = match to_bytes(body, MAX_BODY).await {
		Ok(b) => b,
		Err(e) => return body_error(e, MAX_BODY),
	};
	let (scheme, key): (Scheme, Key) = match (scheme.parse(), key.parse()) {
		(Ok(s), Ok(k)) => (s, k),
		(Err(e), _) | (_, Err(e)) => return error(StatusCode::BAD_REQUEST, e.0),
	};
	let store = panel.store();
	let done = match (method, uri.path().ends_with("/claim")) {
		(Method::GET, false) => store.cached_query(scheme, key).await.map(|kept| match kept {
			Some(stored) => Json(stored).into_response(),
			None => error(StatusCode::NOT_FOUND, "not cached"),
		}),
		(Method::POST, true) => match serde_json::from_slice::<ClaimBody>(&body) {
			Ok(b) => store.claim_query(scheme, key, b.fresh_since, Timestamp::now()).await.map(|c| Json(c).into_response()),
			Err(e) => return error(StatusCode::BAD_REQUEST, format!("not a claim: {e}")),
		},
		(Method::DELETE, true) => store.release_query(scheme, key).await.map(|()| StatusCode::NO_CONTENT.into_response()),
		(Method::PUT, false) => {
			let put: Put = match serde_json::from_slice(&body) {
				Ok(p) => p,
				Err(e) => return error(StatusCode::BAD_REQUEST, format!("not an answer: {e}")),
			};
			if <[u8; 32]>::from(Sha256::digest(&put.request)) != key.0 {
				return error(StatusCode::BAD_REQUEST, "the key is not the SHA-256 of the request");
			}
			store
				.keep_query(scheme, key, &put.answer.to_string(), Some(&put.request), Timestamp::now())
				.await
				.map(|()| StatusCode::NO_CONTENT.into_response())
		}
		(method, claim) => unreachable!("the router mounts no {method} on a {} path", if claim { "claim" } else { "key" }),
	};
	done.unwrap_or_else(|e| {
		crate::report(&e, "the queries cache");
		error(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
	})
}
