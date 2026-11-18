//! An MCP client end to end: registered, authorized with PKCE behind a dev sign-in session, its
//! bearer at `/mcp` and the `/api/v1` gate.

use axum::{
	Router,
	body::Body,
	http::{Method, StatusCode, header},
};
use sa_auth::{Principal, Service};
use sa_panel::{
	import::Export,
	session::pkce_challenge,
	testing::{TestDb, operator, panel},
};
use sa_server::testing::{self, Browser, ORIGIN};
use serde_json::{Value, json};

const VERIFIER: &str = "a-verifier-of-forty-three-or-more-characters-0123456789";
const REDIRECT: &str = "http://127.0.0.1/callback";
/// The client listens on a port of its own (RFC 8252 §7.3).
const LISTENING: &str = "http://127.0.0.1:43117/callback";

fn path_of(location: &str) -> String {
	location.strip_prefix(ORIGIN).unwrap_or(location).to_owned()
}

/// What `/mcp` answers `method` with `params`, as the client holding `bearer`.
async fn rpc(app: &Router, bearer: &str, method: &str, params: Value) -> (StatusCode, Value) {
	let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }).to_string();
	let auth = format!("Bearer {bearer}");
	let (status, _, body) = Browser::default()
		.send_body(
			app,
			Method::POST,
			"/mcp",
			&[("authorization", &auth), ("content-type", "application/json")],
			false,
			Body::from(body),
		)
		.await;
	(status, if body.is_empty() { Value::Null } else { serde_json::from_slice(&body).unwrap() })
}

/// A tool's answer, its text parsed, and whether it is an error.
async fn tool(app: &Router, bearer: &str, name: &str, arguments: Value) -> (Value, bool) {
	let (status, res) = rpc(app, bearer, "tools/call", json!({ "name": name, "arguments": arguments })).await;
	assert_eq!(status, StatusCode::OK, "{res}");
	let result = &res["result"];
	(
		serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap(),
		result["isError"].as_bool().unwrap(),
	)
}

/// A client registered and authorized through the whole OAuth dance: its bearer, the browser of the
/// user who consented, and that browser's own session.
pub(crate) async fn signed_in_client(app: &Router) -> (String, Browser, String) {
	let app = app.clone();
	let mut client = Browser::default();
	let (status, _, body) = client.send_body(&app, Method::GET, "/.well-known/oauth-protected-resource/mcp", &[], false, Body::empty()).await;
	assert_eq!(status, StatusCode::OK);
	let resource: Value = serde_json::from_slice(&body).unwrap();
	assert_eq!(resource["authorization_servers"][0], format!("{ORIGIN}/mcp"));
	let (_, _, body) = client
		.send_body(&app, Method::GET, "/.well-known/oauth-authorization-server/mcp", &[], false, Body::empty())
		.await;
	let server: Value = serde_json::from_slice(&body).unwrap();
	assert_eq!(server["token_endpoint"], format!("{ORIGIN}/mcp/token"));

	let registration = json!({ "redirect_uris": [REDIRECT], "client_name": "Claude Code" }).to_string();
	let (status, _, body) = client
		.send_body(&app, Method::POST, "/mcp/register", &[("content-type", "application/json")], false, Body::from(registration))
		.await;
	assert_eq!(status, StatusCode::CREATED);
	let client_id = serde_json::from_slice::<Value>(&body).unwrap()["client_id"].as_str().unwrap().to_owned();

	// Signed out: the sign-in, then back to the consent page.
	let mut user = Browser::default();
	let authorize = format!(
		"/mcp/authorize?response_type=code&client_id={client_id}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state=xyz",
		url::form_urlencoded::byte_serialize(LISTENING.as_bytes()).collect::<String>(),
		pkce_challenge(VERIFIER),
	);
	let (status, headers) = user.send(&app, Method::GET, &authorize, &[], false).await;
	assert_eq!(status, StatusCode::SEE_OTHER);
	let login = headers[header::LOCATION].to_str().unwrap().to_owned();
	assert!(login.starts_with("/auth/login?return_to="), "{login}");
	let (_, headers) = user.send(&app, Method::GET, &login, &[], false).await;
	let callback = path_of(headers[header::LOCATION].to_str().unwrap());
	let (_, headers) = user.send(&app, Method::GET, &callback, &[], false).await;
	assert_eq!(headers[header::LOCATION], authorize.as_str(), "back where the sign-in began");
	let (status, _, page) = user.send_body(&app, Method::GET, &authorize, &[], false, Body::empty()).await;
	assert_eq!(status, StatusCode::OK);
	assert!(String::from_utf8_lossy(&page).contains("Claude Code wants to act as you"));

	let form = |csrf: &str| {
		url::form_urlencoded::Serializer::new(String::new())
			.extend_pairs([
				("client_id", client_id.as_str()),
				("redirect_uri", LISTENING),
				("code_challenge", &pkce_challenge(VERIFIER)),
				("state", "xyz"),
				("csrf", csrf),
			])
			.finish()
	};
	let urlencoded = [("content-type", "application/x-www-form-urlencoded")];
	let (status, _, _) = user.send_body(&app, Method::POST, "/mcp/authorize", &urlencoded, false, Body::from(form("forged"))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "the form carries the session's CSRF token");
	let csrf = user.jar["sa_csrf"].clone();
	let session = user.jar["sa_session"].clone();
	let (status, headers, _) = user.send_body(&app, Method::POST, "/mcp/authorize", &urlencoded, false, Body::from(form(&csrf))).await;
	assert_eq!(status, StatusCode::FOUND, "to concierge's sign-in, for the client");
	let (status, headers) = user.send(&app, Method::GET, &path_of(headers[header::LOCATION].to_str().unwrap()), &[], false).await;
	assert_eq!(status, StatusCode::SEE_OTHER);
	assert_eq!(user.jar["sa_session"], session, "the browser keeps its own session");
	let back = url::Url::parse(headers[header::LOCATION].to_str().unwrap()).unwrap();
	assert_eq!(format!("{}://{}{}", back.scheme(), back.authority(), back.path()), LISTENING);
	let query: std::collections::HashMap<_, _> = back.query_pairs().into_owned().collect();
	assert_eq!(query["state"], "xyz");

	let exchange = |verifier: &str| {
		url::form_urlencoded::Serializer::new(String::new())
			.extend_pairs([
				("grant_type", "authorization_code"),
				("client_id", client_id.as_str()),
				("code", &query["code"]),
				("code_verifier", verifier),
				("redirect_uri", LISTENING),
			])
			.finish()
	};
	let (status, _, body) = client.send_body(&app, Method::POST, "/mcp/token", &urlencoded, false, Body::from(exchange(VERIFIER))).await;
	assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
	let bearer = serde_json::from_slice::<Value>(&body).unwrap()["access_token"].as_str().unwrap().to_owned();
	let (status, _, _) = client.send_body(&app, Method::POST, "/mcp/token", &urlencoded, false, Body::from(exchange(VERIFIER))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "a code redeems once");

	(bearer, user, session)
}

#[tokio::test]
async fn a_client_signs_in_and_acts_as_its_user() {
	let db = TestDb::create().await;
	let panel = panel(&db).await;
	let (base, upstream) = testing::upstream(Router::new()).await;
	let app = testing::app(panel.clone(), operator(), &base);
	panel
		.import_review_archive(Export {
			people: vec![],
			gmails: vec![],
			gmbs: vec![],
			ledger: vec![],
			kickbacks: vec![],
		})
		.await
		.unwrap()
		.unwrap();

	let (bearer, mut user, session) = signed_in_client(&app).await;

	let (status, init) = rpc(
		&app,
		&bearer,
		"initialize",
		json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "0" } }),
	)
	.await;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
	let (_, listed) = rpc(&app, &bearer, "tools/list", json!({})).await;
	let names: Vec<&str> = listed["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
	assert_eq!(names, ["search", "call"]);

	let (found, _) = tool(&app, &bearer, "search", json!({ "query": "move a GBP" })).await;
	assert!(found["operations"].as_array().unwrap().iter().any(|o| o["id"] == "move_gmb"), "{found}");

	let (me, error) = tool(&app, &bearer, "call", json!({ "operation": "me" })).await;
	assert!(!error, "{me}");
	assert_eq!(me["body"]["email"], "dev@localhost");

	let (added, error) = tool(&app, &bearer, "call", json!({ "operation": "add_trade", "body": { "name": "Plumbing" } })).await;
	assert!(!error, "a write with a bearer needs no CSRF: {added}");
	let trade = added["body"]["id"].clone();
	let (proposal, error) = tool(&app, &bearer, "call", json!({ "operation": "delete_trade", "path": { "id": trade } })).await;
	assert!(!error);
	assert_eq!(proposal["proposed"]["method"], "DELETE", "{proposal}");
	let (trades, _) = tool(&app, &bearer, "call", json!({ "operation": "trades" })).await;
	assert_eq!(trades["body"]["trades"].as_array().unwrap().len(), 1, "nothing ran: {trades}");
	let (deleted, error) = tool(&app, &bearer, "call", json!({ "operation": "delete_trade", "path": { "id": trade }, "confirmed": true })).await;
	assert!(!error, "{deleted}");
	let (trades, _) = tool(&app, &bearer, "call", json!({ "operation": "trades" })).await;
	assert_eq!(trades["body"]["trades"].as_array().unwrap().len(), 0);

	let gate = |auth: String| {
		let app = app.clone();
		async move { Browser::default().send(&app, Method::GET, "/api/v1/me", &[("authorization", &auth)], false).await.0 }
	};
	assert_eq!(gate(format!("Bearer {bearer}")).await, StatusCode::OK);
	assert_eq!(gate("Bearer nope".to_owned()).await, StatusCode::UNAUTHORIZED);
	assert_eq!(gate(format!("Bearer {session}")).await, StatusCode::UNAUTHORIZED, "a browser's session is no bearer");
	let (status, _) = rpc(&app, "nope", "tools/list", json!({})).await;
	assert_eq!(status, StatusCode::UNAUTHORIZED);
	let (status, headers) = Browser::default().send(&app, Method::POST, "/mcp", &[], false).await;
	assert_eq!(status, StatusCode::UNAUTHORIZED);
	assert_eq!(
		headers[header::WWW_AUTHENTICATE],
		format!("Bearer resource_metadata=\"{ORIGIN}/.well-known/oauth-protected-resource/mcp\"").as_str()
	);

	// A service's operation, through the forward: the bearer is the conductor's, never passed up.
	let (status, _) = Browser::default()
		.send(&app, Method::GET, "/api/sites/leads", &[("authorization", &format!("Bearer {bearer}"))], false)
		.await;
	assert_eq!(status, StatusCode::OK);
	let seen = upstream.lock().unwrap().last().cloned().unwrap();
	assert!(!seen.headers.contains_key(header::AUTHORIZATION));
	assert!(matches!(seen.assertion(Service::Sites, None).principal, Principal::User(u) if u.email == "dev@localhost"));

	// Signing out ends every session of the user, the client's too.
	let (status, _) = user.send(&app, Method::POST, "/auth/logout", &[], true).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	assert_eq!(gate(format!("Bearer {bearer}")).await, StatusCode::UNAUTHORIZED);
}

/// An operation a service marks its own tool is one on `/mcp`, `<service>_<operation_id>`: its
/// parameters its input, its text answer handed on as it came; the service's description joins the
/// instructions.
#[tokio::test]
async fn a_services_own_tool_is_listed_and_answers_as_it_came() {
	let db = TestDb::create().await;
	let panel = panel(&db).await;
	let document = json!({
		"openapi": "3.1.0",
		"info": { "title": "playbook", "version": "1", "description": "Answers from the service-arb skill." },
		"servers": [{ "url": "/" }],
		"paths": { "/search": { "get": {
			"operationId": "search",
			"summary": "Searches the playbook.",
			"parameters": [{ "name": "q", "in": "query", "required": true, "schema": { "type": "string" } }],
			"x-tool": true,
		} } },
		"components": { "schemas": {} },
	});
	let routes = Router::new()
		.route("/openapi.json", axum::routing::get(move || std::future::ready(axum::Json(document.clone()))))
		.route(
			"/search",
			axum::routing::get(|axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>| async move {
				([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], format!("# Found\n\n\"{}\" is in chapter 3.", q["q"]))
			}),
		);
	let (base, seen) = testing::upstream(routes).await;
	let (sign_in, forward) = testing::conductor(panel.clone(), operator(), "dev@localhost", &base.parse().unwrap());
	forward.catalog().follow(testing::services(&base.parse().unwrap()), tokio::sync::watch::channel(true).1).await;
	let app = sa_server::http::app(sign_in, Some(forward), None, sa_server::http::Limits::default(), None, None, None, None, None);
	let (bearer, _, _) = signed_in_client(&app).await;

	let (_, init) = rpc(&app, &bearer, "initialize", json!({ "protocolVersion": "2025-06-18" })).await;
	let instructions = init["result"]["instructions"].as_str().unwrap();
	assert!(instructions.starts_with("The Service-Arb panel"), "{instructions}");
	assert!(instructions.contains("Answers from the service-arb skill."), "{instructions}");
	let (_, listed) = rpc(&app, &bearer, "tools/list", json!({})).await;
	let own = listed["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "playbook_search").cloned().unwrap();
	assert_eq!(own["inputSchema"]["required"], json!(["q"]), "{own}");
	assert_eq!(own["annotations"]["readOnlyHint"], true);

	let (status, res) = rpc(&app, &bearer, "tools/call", json!({ "name": "playbook_search", "arguments": { "q": "a GBP's name" } })).await;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(res["result"]["content"][0]["text"], "# Found\n\n\"a GBP's name\" is in chapter 3.", "{res}");
	let asked = seen.lock().unwrap().iter().rev().find(|s| s.path_and_query.starts_with("/search")).cloned().unwrap();
	assert_eq!(asked.path_and_query, "/search?q=a+GBP%27s+name");
	assert!(matches!(asked.assertion(Service::Playbook, None).principal, Principal::User(u) if u.email == "dev@localhost"));
}
