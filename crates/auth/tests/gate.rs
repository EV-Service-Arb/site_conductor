//! A Route row's gate, mounted as a service mounts its table: one assertion opens its one request,
//! and the row's permission is what opens it.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
	Router,
	body::Body,
	extract::FromRef,
	http::{Method, Request, StatusCode, request::Parts},
	response::Response,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use ev_lib::api::{Refusal, Route};
use sa_auth::{Assertion, Conductor, Denied, Gate, Principal, Service, Signer, TTL, User, Verifier, Vouch, Work};
use tower::ServiceExt as _;

#[derive(Clone)]
struct Sites(Verifier);

impl FromRef<Sites> for Verifier {
	fn from_ref(s: &Sites) -> Self {
		s.0.clone()
	}
}

/// Says who reads leads.
async fn leads(user: User) -> String {
	user.email
}

/// Says who reads the document, when it is a person.
async fn document(user: Option<User>) -> String {
	user.map_or("conductor".into(), |u| u.email)
}

async fn feed(_: Conductor) -> String {
	"feed".into()
}

fn table<S: Clone + Send + Sync + 'static>() -> Vec<Route<S, Gate>>
where
	Gate: ev_lib::api::Guard<S>, {
	vec![
		Route::new(Method::GET, "/leads", Gate::USER.needs(Work::Read), leads),
		Route::new(Method::GET, "/openapi.json", Gate::USER.or_conductor(), document),
	]
}

fn mount<S: Clone + Send + Sync + 'static>(routes: Vec<Route<S, Gate>>) -> Router<S> {
	routes.into_iter().fold(Router::new(), |r, route| r.route(route.path, route.handler()))
}

fn signer() -> Signer {
	let mut seed = [0u8; 32];
	getrandom::fill(&mut seed).unwrap();
	format!("k1:{}", STANDARD.encode(seed)).parse().unwrap()
}

fn now() -> i64 {
	SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

fn user(permissions: &[&str]) -> Principal {
	Principal::User(sa_auth::User {
		sub: "0192".into(),
		email: "a@b.c".into(),
		email_verified: true,
		name: "A".into(),
		permissions: permissions.iter().copied().collect(),
	})
}

async fn get(app: &Router, path: &str, assertion: Option<(&Signer, Assertion)>) -> (StatusCode, String) {
	let mut req = Request::get(path);
	if let Some((signer, a)) = assertion {
		req = req.header(sa_auth::HEADER, signer.sign(&a));
	}
	let res: Response = app.clone().oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
	let status = res.status();
	let body = axum::body::to_bytes(res.into_body(), 1 << 16).await.unwrap();
	(status, String::from_utf8(body.to_vec()).unwrap())
}

fn assertion(principal: Principal, aud: Service, method: &str, path: &str, exp: i64) -> Assertion {
	Assertion {
		aud,
		principal,
		method: method.into(),
		path: path.into(),
		exp,
	}
}

#[tokio::test]
async fn an_assertion_opens_its_one_request_and_the_rows_permission_opens_the_route() {
	let panel = signer();
	let state = Sites(Verifier::new(panel.public().parse().unwrap(), Service::Sites));
	let app: Router = Router::new()
		.nest("/api/v1", mount(table()))
		.route("/api/conductor/feed", axum::routing::get(feed))
		.with_state(state);
	let worker = || user(&[Work::Read.as_str()]);
	let check = |principal: Principal, aud, method: &'static str, signed: &'static str, exp, asked: &'static str| {
		let app = app.clone();
		let panel = &panel;
		async move { get(&app, asked, Some((panel, assertion(principal, aud, method, signed, exp)))).await.0 }
	};
	let (n, s) = (now(), Service::Sites);
	let leads = "/api/v1/leads";

	assert_eq!(get(&app, leads, None).await.0, StatusCode::UNAUTHORIZED);
	assert_eq!(
		get(&app, leads, Some((&panel, assertion(worker(), s, "GET", leads, n + TTL)))).await,
		(StatusCode::OK, "a@b.c".into())
	);
	assert_eq!(
		check(worker(), s, "GET", "/leads", n + TTL, leads).await,
		StatusCode::UNAUTHORIZED,
		"the path the conductor asked, not the nest's"
	);
	assert_eq!(check(worker(), s, "POST", leads, n + TTL, leads).await, StatusCode::UNAUTHORIZED);
	assert_eq!(check(user(&[]), Service::ReviewArchive, "GET", leads, n + TTL, leads).await, StatusCode::UNAUTHORIZED);
	assert_eq!(check(worker(), s, "GET", leads, n, leads).await, StatusCode::UNAUTHORIZED);
	assert_eq!(
		get(&app, leads, Some((&signer(), assertion(worker(), s, "GET", leads, n + TTL)))).await.0,
		StatusCode::UNAUTHORIZED,
		"a key the service does not hold"
	);

	let (status, body) = get(&app, leads, Some((&panel, assertion(user(&[]), s, "GET", leads, n + TTL)))).await;
	assert_eq!(status, StatusCode::FORBIDDEN);
	assert!(body.contains(r#""code":"forbidden""#), "{body}");

	assert_eq!(check(Principal::Conductor, s, "GET", leads, n + TTL, leads).await, StatusCode::UNAUTHORIZED);
	let doc = "/api/v1/openapi.json";
	assert_eq!(
		get(&app, doc, Some((&panel, assertion(Principal::Conductor, s, "GET", doc, n + TTL)))).await,
		(StatusCode::OK, "conductor".into())
	);
	assert_eq!(
		get(&app, doc, Some((&panel, assertion(user(&[]), s, "GET", doc, n + TTL)))).await,
		(StatusCode::OK, "a@b.c".into())
	);

	let feed = "/api/conductor/feed";
	assert_eq!(check(Principal::Conductor, s, "GET", feed, n + TTL, feed).await, StatusCode::OK);
	assert_eq!(check(worker(), s, "GET", feed, n + TTL, feed).await, StatusCode::UNAUTHORIZED);
}

/// The conductor's case: its own session says who is calling, the gate is the same.
#[derive(Clone)]
struct Session(Principal);

impl Vouch<()> for Session {
	type Refusal = Denied;

	const REFUSALS: &'static [Denied] = &[];

	async fn vouch(&self, _: &Gate, _: &mut Parts) -> Result<Principal, Refusal<Denied>> {
		Ok(self.0.clone())
	}
}

#[tokio::test]
async fn a_state_that_vouches_by_itself_is_gated_by_the_same_rows() {
	let app = |principal| Router::new().merge(mount(table())).with_state(Session(principal));
	assert_eq!(get(&app(user(&[Work::Read.as_str()])), "/leads", None).await.0, StatusCode::OK);
	assert_eq!(get(&app(user(&[])), "/leads", None).await.0, StatusCode::FORBIDDEN);
}

#[test]
fn the_document_names_what_opens_a_route_and_what_its_gate_answers() {
	let docs: ev_lib::api::Docs = &[("gate::leads", "Says who reads leads."), ("gate::document", "Says who reads the document.")];
	let doc = serde_json::to_value(ev_lib::api::document::<Sites, _>("sites", "0", "/api/v1", &table(), docs, None)).unwrap();
	let op = |path: &str| doc["paths"][path]["get"].clone();
	assert_eq!(op("/leads")["x-permission"], Work::Read.as_str());
	assert_eq!(op("/leads")["x-errors"], serde_json::json!({ "unauthenticated": 401, "forbidden": 403 }));
	assert_eq!(op("/openapi.json")["x-permission"], serde_json::Value::Null);
	assert_eq!(
		op("/openapi.json")["x-errors"],
		serde_json::json!({ "unauthenticated": 401 }),
		"a gate naming no permission forbids nothing"
	);
}
