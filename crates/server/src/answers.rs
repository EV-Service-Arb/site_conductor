//! The answers store over HTTP (`sa_core::answers`), for the services and machines that buy
//! answers: signed as ingest is, the MAC over the method, the path and the body. An `answers` key
//! only; any other is refused as an unknown one is.
//!
//! ```text
//! GET  /api/answers/{scheme}/{key}         → 200 Stored | 404
//! POST /api/answers/{scheme}/{key}/claim   ClaimBody → 200 Claim
//! DELETE /api/answers/{scheme}/{key}/claim → 204: the buy failed, the claim given up
//! PUT  /api/answers/{scheme}/{key}         Put → 204; 400 a request the key is not the digest of
//! ```

use axum::{
	Json, Router,
	body::{Body, to_bytes},
	error_handling::HandleErrorLayer,
	extract::{OriginalUri, Path, State},
	http::{HeaderMap, Method, StatusCode},
	response::{IntoResponse, Response},
	routing::{get, post},
};
use jiff::Timestamp;
use sa_auth::SourceKind;
use sa_core::answers::{ClaimBody, Key, Put, Scheme};
use sa_panel::{Panel, SignedBatch};
use sha2::{Digest, Sha256};
use tower::{BoxError, ServiceBuilder};
use tower_http::timeout::{RequestBodyTimeoutLayer, TimeoutLayer};

use crate::{
	http::Limits,
	ingest::{REFUSED_MESSAGE, body_error, error, refused, signed, unsigned},
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
		.route("/api/answers/{scheme}/{key}", get(answer).put(answer).layer(layers.clone()))
		.route("/api/answers/{scheme}/{key}/claim", post(answer).delete(answer).layer(layers))
		.with_state(panel)
}

/// Every route: an `answers` key's request, then what its method asks of the store.
async fn answer(State(panel): State<Panel>, Path((scheme, key)): Path<(String, String)>, method: Method, OriginalUri(uri): OriginalUri, headers: HeaderMap, body: Body) -> Response {
	let Some((key_id, timestamp, signature)) = signed(&headers) else {
		return unsigned();
	};
	let body = match to_bytes(body, MAX_BODY).await {
		Ok(b) => b,
		Err(e) => return body_error(e, MAX_BODY),
	};
	let message = sa_core::answers::signed(method.as_str(), uri.path(), &body);
	let request = SignedBatch {
		key_id,
		timestamp,
		signature,
		body: &message,
	};
	match panel.authenticate(request, Timestamp::now()).await {
		Ok(g) if g.kind == SourceKind::Answers => {}
		Ok(_) => return error(StatusCode::UNAUTHORIZED, REFUSED_MESSAGE),
		Err(e) => return refused(e),
	}
	let (scheme, key): (Scheme, Key) = match (scheme.parse(), key.parse()) {
		(Ok(s), Ok(k)) => (s, k),
		(Err(e), _) | (_, Err(e)) => return error(StatusCode::BAD_REQUEST, e.0),
	};
	let store = panel.store();
	let done = match (method, uri.path().ends_with("/claim")) {
		(Method::GET, false) => store.answer(scheme, key).await.map(|kept| match kept {
			Some(stored) => Json(stored).into_response(),
			None => error(StatusCode::NOT_FOUND, "no answer kept"),
		}),
		(Method::POST, true) => match serde_json::from_slice::<ClaimBody>(&body) {
			Ok(b) => store.claim_answer(scheme, key, b.fresh_since, Timestamp::now()).await.map(|c| Json(c).into_response()),
			Err(e) => return error(StatusCode::BAD_REQUEST, format!("not a claim: {e}")),
		},
		(Method::DELETE, true) => store.release_answer_claim(scheme, key).await.map(|()| StatusCode::NO_CONTENT.into_response()),
		(Method::PUT, false) => {
			let put: Put = match serde_json::from_slice(&body) {
				Ok(p) => p,
				Err(e) => return error(StatusCode::BAD_REQUEST, format!("not an answer: {e}")),
			};
			if <[u8; 32]>::from(Sha256::digest(&put.request)) != key.0 {
				return error(StatusCode::BAD_REQUEST, "the key is not the SHA-256 of the request");
			}
			store
				.put_answer(scheme, key, &put.answer.to_string(), Some(&put.request), Timestamp::now())
				.await
				.map(|()| StatusCode::NO_CONTENT.into_response())
		}
		(method, claim) => unreachable!("the router mounts no {method} on a {} path", if claim { "claim" } else { "key" }),
	};
	done.unwrap_or_else(|e| {
		crate::report(&e, "the answers store");
		error(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
	})
}
