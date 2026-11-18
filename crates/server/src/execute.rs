//! `execute`: an agent's code, run in sa_sandbox's pod, and `/api/run`, the one way back from it
//! (docs/ARCHITECTURE.md, Code mode). A run's token is a handle to its caller's own credentials,
//! held here while the run lasts; what it may call is the catalog's operations, and none that
//! destroys or spends: the first such call stops the run and is proposed.

use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
	time::Duration,
};

use axum::{
	Router,
	body::to_bytes,
	extract::{Request, State},
	http::{HeaderMap, StatusCode, header},
	response::{IntoResponse, Response},
	routing::any,
};
use eyre::WrapErr;
use jiff::Timestamp;
use sa_auth::{Assertion, Principal, Service, Signer};
use sa_sandbox::{CALLS, LIMIT, REFUSED, Ran, Run};
use serde_json::{Map, Value};

use crate::tools::{self, Call, Tools};

/// Where a run's requests come back, in-cluster only.
pub const PREFIX: &str = "/api/run";

/// The runner, and the runs in flight.
#[derive(Clone)]
pub struct Sandbox {
	base: String,
	signer: Arc<Signer>,
	http: reqwest::Client,
	runs: Arc<Mutex<HashMap<String, Held>>>,
}
impl Sandbox {
	pub fn try_new(runner: &str, signer: Signer) -> eyre::Result<Self> {
		let _ = rustls::crypto::ring::default_provider().install_default(); //IGNORED_ERROR: Err is "a provider is already installed", which is all reqwest needs
		Ok(Self {
			base: runner.trim_end_matches('/').to_owned(),
			signer: Arc::new(signer),
			// past the runner's own limit: its answer, a killed run's included, comes first
			http: reqwest::Client::builder().timeout(LIMIT + Duration::from_secs(15)).build().wrap_err("the sandbox's client")?,
			runs: Arc::default(),
		})
	}

	/// Runs `code` as the caller `credentials` are; its token lives exactly as long as this does.
	pub(crate) async fn execute(&self, credentials: HeaderMap, code: String) -> Result<Executed, String> {
		let mut bytes = [0; 32];
		getrandom::fill(&mut bytes).expect("the OS has randomness");
		let token = hex::encode(bytes);
		let run = InFlight {
			runs: &self.runs,
			token: token.clone(),
		};
		self.runs.lock().expect("never held across a panic").insert(
			token.clone(),
			Held {
				credentials,
				calls: 0,
				proposed: None,
			},
		);
		let assertion = self.signer.sign(&Assertion {
			aud: Service::Sandbox,
			principal: Principal::Conductor,
			method: "POST".into(),
			path: "/runs".into(),
			exp: Timestamp::now().as_second() + sa_auth::TTL,
		});
		let res = self
			.http
			.post(format!("{}/runs", self.base))
			.header(sa_auth::HEADER, assertion)
			.json(&Run { code, token })
			.send()
			.await
			.map_err(|e| format!("the sandbox did not answer: {e}"))?;
		let status = res.status();
		let text = res.text().await.map_err(|e| format!("the sandbox's answer broke off: {e}"))?;
		let ran: Ran = match status {
			StatusCode::OK => serde_json::from_str(&text).expect("the runner answers a Ran"),
			StatusCode::SERVICE_UNAVAILABLE => return Err(format!("the sandbox is busy: {text}")),
			s => return Err(format!("the sandbox answered {s}: {text}")),
		};
		let proposed = run.held(|h| h.proposed.take());
		Ok(Executed { ran, proposed })
	}

	/// `/api/run/*`: a run's requests, as the caller's own once the gate lets them by.
	pub(crate) fn routes(tools: Tools) -> Router {
		Router::new().route(&format!("{PREFIX}/{{*path}}"), any(relay)).with_state(tools)
	}
}

/// What a run did, and the call it stopped at to propose.
pub struct Executed {
	pub ran: Ran,
	pub proposed: Option<Call>,
}

/// A run in flight, by its token.
struct Held {
	credentials: HeaderMap,
	calls: usize,
	proposed: Option<Call>,
}

/// Drops the run's token however its `execute` ends.
struct InFlight<'a> {
	runs: &'a Mutex<HashMap<String, Held>>,
	token: String,
}

impl InFlight<'_> {
	fn held<T>(&self, f: impl FnOnce(&mut Held) -> T) -> T {
		f(self.runs.lock().expect("never held across a panic").get_mut(&self.token).expect("removed only on drop"))
	}
}

impl Drop for InFlight<'_> {
	fn drop(&mut self) {
		self.runs.lock().expect("never held across a panic").remove(&self.token);
	}
}

/// The gate's own refusal: the run stops at it ([`REFUSED`]).
fn refused(status: StatusCode, why: String) -> Response {
	(status, [(REFUSED, "1")], why).into_response()
}

async fn relay(State(tools): State<Tools>, req: Request) -> Response {
	let sandbox = tools.sandbox().expect("mounted with the sandbox");
	let token = req
		.headers()
		.get(header::AUTHORIZATION)
		.and_then(|v| v.to_str().ok())
		.and_then(|v| v.strip_prefix("Bearer "))
		.map(str::to_owned);
	let token = token.as_deref();
	let path = req.uri().path().strip_prefix(PREFIX).expect("mounted under PREFIX").to_owned();
	let method = req.method().clone();
	let credentials = {
		let mut runs = sandbox.runs.lock().expect("never held across a panic");
		let Some(held) = token.and_then(|t| runs.get_mut(t)) else {
			return refused(StatusCode::UNAUTHORIZED, "no run holds this token: it ended".into());
		};
		if held.proposed.is_some() {
			return refused(StatusCode::FORBIDDEN, "the run stopped at its proposal".into());
		}
		held.calls += 1;
		if held.calls > CALLS {
			return refused(StatusCode::TOO_MANY_REQUESTS, format!("a run calls at most {CALLS} operations"));
		}
		held.credentials.clone()
	};
	let Some((op, params)) = tools.catalog().operation(&method, &path) else {
		return refused(StatusCode::NOT_FOUND, format!("{method} {path} is no operation of the catalog's"));
	};
	let id = op.operation_id.clone().expect("every operation has an id");
	let query: Map<String, Value> = url::form_urlencoded::parse(req.uri().query().unwrap_or_default().as_bytes())
		.map(|(k, v)| (k.into_owned(), Value::String(v.into_owned())))
		.collect();
	let hints = tools::hints(method.as_str().to_lowercase().as_str(), &op);
	let (parts, body) = req.into_parts();
	if hints.proposed() {
		let json = parts.headers.get(header::CONTENT_TYPE).is_some_and(|t| t.as_bytes().starts_with(b"application/json"));
		let body = match to_bytes(body, tools::ANSWER).await {
			Ok(b) if b.is_empty() => None,
			Ok(b) if json => match serde_json::from_slice(&b) {
				Ok(v) => Some(v),
				Err(e) => return refused(StatusCode::BAD_REQUEST, format!("{id}'s body is no JSON: {e}")),
			},
			Ok(_) => return refused(StatusCode::FORBIDDEN, format!("{id} destroys or spends, and its body is no JSON to propose: call it with `call`")),
			Err(e) => return refused(StatusCode::BAD_REQUEST, format!("{id}'s body: {e}")),
		};
		let call = Call {
			operation: id.clone(),
			path: Some(params),
			query: Some(query),
			body,
		};
		let mut runs = sandbox.runs.lock().expect("never held across a panic");
		let Some(held) = token.and_then(|t| runs.get_mut(t)) else {
			return refused(StatusCode::UNAUTHORIZED, "no run holds this token: it ended".into());
		};
		held.proposed = Some(call);
		return refused(
			StatusCode::FORBIDDEN,
			format!("{id} destroys or spends: not run here. The run stops, and it is proposed to the person."),
		);
	}
	let uri = parts.uri.query().map_or_else(|| path.clone(), |q| format!("{path}?{q}"));
	let mut inner = Request::builder().method(method).uri(uri);
	for name in [header::CONTENT_TYPE, header::ACCEPT] {
		if let Some(v) = parts.headers.get(&name) {
			inner = inner.header(name, v);
		}
	}
	let mut res = tools.send(&credentials, inner, body).await;
	// a rotated session's cookie is the caller's alone
	res.headers_mut().remove(header::SET_COOKIE);
	res
}
