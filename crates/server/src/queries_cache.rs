//! The queries cache over HTTP (`sa_core::queries_cache`), open to everyone who might run the same
//! query, and its buy, for the person review_archive serves: bounded as ingest is, and rate-limited
//! per client at the edge.
//!
//! ```text
//! GET    /api/queries_cache/{scheme}/{key}         → 200 Stored | 404
//! POST   /api/queries_cache/{scheme}/{key}/claim   ClaimBody → 200 Claim
//! DELETE /api/queries_cache/{scheme}/{key}/claim   → 204: the buy failed, the claim given up
//! PUT    /api/queries_cache/{scheme}/{key}         Put → 204; 400 a request the key is not the digest of
//! POST   /api/queries_cache/places/{key}/buy       BuyBody, x-sa-assertion → 200 {answer} | BuyRefused
//! ```

use std::sync::Arc;

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
use sa_core::{
	queries_cache::{BuyBody, BuyRefused, ClaimBody, Key, Put, Scheme},
	usage::Bill,
};
use sa_panel::Panel;
use tower::{BoxError, ServiceBuilder};
use tower_http::timeout::{RequestBodyTimeoutLayer, TimeoutLayer};

use crate::{
	http::Limits,
	ingest::{body_error, error},
};

/// A DataForSEO expansion runs to a few MiB.
const MAX_BODY: usize = 8 * 1024 * 1024;

pub(crate) fn routes(panel: Panel, limits: &Limits, vouchers: Option<sa_auth::Keys>) -> Router {
	let layers = ServiceBuilder::new()
		.layer(HandleErrorLayer::new(|_: BoxError| async { error(StatusCode::SERVICE_UNAVAILABLE, "busy, try again") }))
		.load_shed()
		.concurrency_limit(limits.max_concurrent)
		.layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, limits.request_timeout))
		.layer(RequestBodyTimeoutLayer::new(limits.body_timeout));
	let buys = Router::new()
		.route("/api/queries_cache/places/{key}/buy", post(buy).layer(layers.clone()))
		.with_state((panel.clone(), vouchers.map(Arc::new)));
	Router::new()
		.route("/api/queries_cache/{scheme}/{key}", get(query).put(query).layer(layers.clone()))
		.route("/api/queries_cache/{scheme}/{key}/claim", post(query).delete(query).layer(layers))
		.with_state(panel)
		.merge(buys)
}

/// A Places request bought, or served as kept, for the person the conductor's assertion on
/// review_archive's request names (`sa_auth::vouched`): theirs the keys, theirs the tokens.
async fn buy(State((panel, vouchers)): State<(Panel, Option<Arc<sa_auth::Keys>>)>, Path(key): Path<String>, headers: HeaderMap, body: Body) -> Response {
	let refuse = |refused: BuyRefused| {
		let status = match refused {
			BuyRefused::Invalid { .. } => StatusCode::BAD_REQUEST,
			BuyRefused::Unauthenticated { .. } => StatusCode::UNAUTHORIZED,
			BuyRefused::Unpaid { .. } => StatusCode::PAYMENT_REQUIRED,
			BuyRefused::Places { .. } => StatusCode::BAD_GATEWAY,
			BuyRefused::Unbought | BuyRefused::Unavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
		};
		(status, Json(refused)).into_response()
	};
	let unauthenticated = |why: String| refuse(BuyRefused::Unauthenticated { why });
	let body = match to_bytes(body, MAX_BODY).await {
		Ok(b) => b,
		Err(e) => return body_error(e, MAX_BODY),
	};
	let key: Key = match key.parse() {
		Ok(k) => k,
		Err(e) => return refuse(BuyRefused::Invalid { why: e.0 }),
	};
	let Some(vouchers) = vouchers else {
		return unauthenticated("this conductor signs no assertion for a service to hand back".into());
	};
	let Some(token) = headers.get(sa_auth::HEADER).and_then(|t| t.to_str().ok()) else {
		return unauthenticated(format!("no {}", sa_auth::HEADER));
	};
	let user = match sa_auth::vouched(&vouchers, token, sa_auth::Service::ReviewArchive, Timestamp::now().as_second()) {
		Ok(user) => user,
		Err(e) => return unauthenticated(format!("the assertion is refused: {e}")),
	};
	let user_id: uuid::Uuid = user.sub.parse().expect("the conductor signs a person's user id as their sub");
	let body: BuyBody = match serde_json::from_slice(&body) {
		Ok(b) => b,
		Err(e) => return refuse(BuyRefused::Invalid { why: format!("not a buy: {e}") }),
	};
	if Key::of(&body.request) != key {
		return refuse(BuyRefused::Invalid {
			why: "the key is not the request's SHA-256".into(),
		});
	}
	match panel.buy_for(user_id, &body).await {
		Ok(Ok(answer)) => Json(serde_json::json!({ "answer": answer })).into_response(),
		Ok(Err(refused)) => refuse(refused),
		Err(e) => {
			crate::report(&e, "a buy of the queries cache");
			error(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
		}
	}
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
			if let Some(b) = &put.bought {
				match (b.bill, b.item.meter().is_some(), &b.account) {
					(Bill::Theirs, _, None) | (Bill::Ours, false, None) => {}
					(Bill::Ours, true, Some(a)) if panel.accounts().get(a).is_some() => {}
					(Bill::Ours, true, Some(a)) => return error(StatusCode::BAD_REQUEST, format!("{a:?} is no Google account of the conductor's")),
					(Bill::Ours, true, None) => return error(StatusCode::BAD_REQUEST, "a buy on our bill names the Google account that paid"),
					(Bill::Ours, false, Some(_)) => return error(StatusCode::BAD_REQUEST, format!("{} draws on no Google account", b.item.tool_title())),
					(Bill::Theirs, _, Some(_)) => return error(StatusCode::BAD_REQUEST, "a buy on a person's own key names no account"),
				}
			}
			store
				.keep_query(scheme, key, &put.answer.to_string(), &put.request, put.bought.map(|b| (b, scheme.as_str())), Timestamp::now())
				.await
				.map(|kept| match kept {
					Ok(()) => StatusCode::NO_CONTENT.into_response(),
					Err(refused) => error(StatusCode::BAD_REQUEST, refused.to_string()),
				})
		}
		(method, claim) => unreachable!("the router mounts no {method} on a {} path", if claim { "claim" } else { "key" }),
	};
	done.unwrap_or_else(|e| {
		crate::report(&e, "the queries cache");
		error(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
	})
}
