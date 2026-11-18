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
	testing::{TestDb, operator, panel},
};
use sa_server::testing::{self, Browser, Mcp, ORIGIN, mcp_grant};
use serde_json::{Value, json};

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

	let (bearer, mut user, session) = mcp_grant(&app, "ask").await;

	let (init, client) = Mcp::initialize(&app, &bearer, json!({})).await;
	assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
	let (_, listed) = client.rpc(&app, "tools/list", json!({})).await;
	let names: Vec<&str> = listed["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
	assert_eq!(names, ["search", "call"]);

	let (found, _) = client.tool(&app, "search", json!({ "query": "move a GBP" })).await;
	assert!(found["operations"].as_array().unwrap().iter().any(|o| o["id"] == "move_gmb"), "{found}");

	let (me, error) = client.tool(&app, "call", json!({ "operation": "me" })).await;
	assert!(!error, "{me}");
	assert_eq!(me["body"]["email"], "dev@localhost");

	let (added, error) = client.tool(&app, "call", json!({ "operation": "add_trade", "body": { "name": "Plumbing" } })).await;
	assert!(!error, "a write with a bearer needs no CSRF: {added}");
	let trade = added["body"]["id"].clone();
	let (proposal, error) = client.tool(&app, "call", json!({ "operation": "delete_trade", "path": { "id": trade } })).await;
	assert!(!error);
	assert_eq!(proposal["proposed"]["method"], "DELETE", "{proposal}");
	let (trades, _) = client.tool(&app, "call", json!({ "operation": "trades" })).await;
	assert_eq!(trades["body"]["trades"].as_array().unwrap().len(), 1, "nothing ran: {trades}");
	let (deleted, error) = client.tool(&app, "call", json!({ "operation": "delete_trade", "path": { "id": trade }, "confirmed": true })).await;
	assert!(!error, "{deleted}");
	let (trades, _) = client.tool(&app, "call", json!({ "operation": "trades" })).await;
	assert_eq!(trades["body"]["trades"].as_array().unwrap().len(), 0);

	let gate = |auth: String| {
		let app = app.clone();
		async move { Browser::default().send(&app, Method::GET, "/api/v1/me", &[("authorization", &auth)], false).await.0 }
	};
	assert_eq!(gate(format!("Bearer {bearer}")).await, StatusCode::OK);
	assert_eq!(gate("Bearer nope".to_owned()).await, StatusCode::UNAUTHORIZED);
	assert_eq!(gate(format!("Bearer {session}")).await, StatusCode::UNAUTHORIZED, "a browser's session is no bearer");
	let (status, _) = Mcp { bearer: "nope".into(), session: client.session.clone() }.rpc(&app, "tools/list", json!({})).await;
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
	let app = sa_server::http::app(sign_in, Some(forward), None, sa_server::http::Limits::default(), None, None, None, None);
	let (bearer, _, _) = mcp_grant(&app, "ask").await;

	let (init, client) = Mcp::initialize(&app, &bearer, json!({})).await;
	let instructions = init["result"]["instructions"].as_str().unwrap();
	assert!(instructions.starts_with("The Service-Arb panel"), "{instructions}");
	assert!(instructions.contains("Answers from the service-arb skill."), "{instructions}");
	let (_, listed) = client.rpc(&app, "tools/list", json!({})).await;
	let own = listed["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "playbook_search").cloned().unwrap();
	assert_eq!(own["inputSchema"]["required"], json!(["q"]), "{own}");
	assert_eq!(own["annotations"]["readOnlyHint"], true);

	let (status, res) = client.rpc(&app, "tools/call", json!({ "name": "playbook_search", "arguments": { "q": "a GBP's name" } })).await;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(res["result"]["content"][0]["text"], "# Found\n\n\"a GBP's name\" is in chapter 3.", "{res}");
	let asked = seen.lock().unwrap().iter().rev().find(|s| s.path_and_query.starts_with("/search")).cloned().unwrap();
	assert_eq!(asked.path_and_query, "/search?q=a+GBP%27s+name");
	assert!(matches!(asked.assertion(Service::Playbook, None).principal, Principal::User(u) if u.email == "dev@localhost"));
}

/// A grant's level is its scope: granted `spend`, a spending call runs without `confirmed`, and one
/// that removes is still proposed.
#[tokio::test]
async fn a_grant_with_the_spend_scope_spends_on_its_own() {
	let db = TestDb::create().await;
	let panel = panel(&db).await;
	let (base, _) = testing::upstream(Router::new()).await;
	let app = testing::app(panel.clone(), operator(), &base);
	let (status, _, body) = Browser::default().send_body(&app, Method::GET, "/.well-known/oauth-protected-resource/mcp", &[], false, Body::empty()).await;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["scopes_supported"], json!(["spend", "act"]));
	let (bearer, _, _) = mcp_grant(&app, "spend").await;
	let (_, client) = Mcp::initialize(&app, &bearer, json!({})).await;

	let (_, listed) = client.rpc(&app, "tools/list", json!({})).await;
	let call = listed["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "call").cloned().unwrap();
	assert!(call["description"].as_str().unwrap().contains("spends runs"), "{call}");
	let (spent, _) = client.tool(&app, "call", json!({ "operation": "buy_map", "body": { "account": "nope" } })).await;
	assert!(spent.get("proposed").is_none() && spent.get("status").is_some(), "ran: {spent}");
	let (removed, _) = client.tool(&app, "call", json!({ "operation": "delete_trade", "path": { "id": 7 } })).await;
	assert_eq!(removed["proposed"]["method"], "DELETE", "{removed}");
}

/// `initialize` opens a session every later request names; one it does not know is 404, so the client
/// initializes anew, and the client's DELETE ends it.
#[tokio::test]
async fn a_client_works_in_the_session_it_opened() {
	let db = TestDb::create().await;
	let panel = panel(&db).await;
	let (base, _) = testing::upstream(Router::new()).await;
	let app = testing::app(panel.clone(), operator(), &base);
	let (bearer, _, _) = mcp_grant(&app, "ask").await;
	let (_, client) = Mcp::initialize(&app, &bearer, json!({})).await;
	assert_eq!(client.rpc(&app, "ping", json!({})).await.0, StatusCode::OK);
	let auth = format!("Bearer {bearer}");
	let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }).to_string();
	let headers = [("authorization", auth.as_str()), ("content-type", "application/json")];
	let (status, ..) = Browser::default().send_body(&app, Method::POST, "/mcp", &headers, false, Body::from(ping.clone())).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "no session named");
	let (_, other) = Mcp::initialize(&app, &bearer, json!({})).await;
	assert_ne!(other.session, client.session);
	let (status, ..) = Browser::default()
		.send_body(&app, Method::DELETE, "/mcp", &[("authorization", &auth), ("mcp-session-id", &client.session)], false, Body::empty())
		.await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	assert_eq!(client.rpc(&app, "ping", json!({})).await.0, StatusCode::NOT_FOUND, "ended: initialize anew");
	assert_eq!(other.rpc(&app, "ping", json!({})).await.0, StatusCode::OK);
}

/// A client that declared `elicitation` asks its user what the grant holds: the call waits on the
/// answer's stream for them, runs when they accept, and not when they decline; `confirmed` is
/// nobody's word there.
#[tokio::test]
async fn a_client_that_elicits_asks_its_user_before_what_the_grant_holds() {
	let db = TestDb::create().await;
	let panel = panel(&db).await;
	let (base, _) = testing::upstream(Router::new()).await;
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
	let (bearer, _, _) = mcp_grant(&app, "ask").await;
	let (_, client) = Mcp::initialize(&app, &bearer, json!({ "elicitation": {} })).await;
	let (_, listed) = client.rpc(&app, "tools/list", json!({})).await;
	let call = listed["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "call").cloned().unwrap();
	assert!(call["inputSchema"]["properties"].get("confirmed").is_none(), "{call}");
	let (_, refused) = client.rpc(&app, "tools/call", json!({ "name": "call", "arguments": { "operation": "trades", "confirmed": true } })).await;
	assert!(refused["error"]["message"].as_str().unwrap().contains("asks its user itself"), "{refused}");

	let (added, _) = client.tool(&app, "call", json!({ "operation": "add_trade", "body": { "name": "Plumbing" } })).await;
	let trade = added["body"]["id"].clone();
	let mut events = client.tool_streamed(&app, "call", json!({ "operation": "delete_trade", "path": { "id": trade } })).await;
	let asked = events.next().await.unwrap();
	assert_eq!(asked["method"], "elicitation/create", "{asked}");
	assert_eq!(asked["params"]["requestedSchema"]["properties"]["approve"]["type"], "boolean");
	assert!(asked["params"]["message"].as_str().unwrap().contains("DELETE"), "{asked}");
	let (trades, _) = client.tool(&app, "call", json!({ "operation": "trades" })).await;
	assert_eq!(trades["body"]["trades"].as_array().unwrap().len(), 1, "nothing ran while it waits");
	assert_eq!(client.answer(&app, &asked["id"], json!({ "action": "decline" })).await, StatusCode::ACCEPTED);
	let (declined, _) = testing::tool_result(&events.next().await.unwrap());
	assert!(declined["declined"].is_string(), "{declined}");
	assert!(events.next().await.is_none());

	let mut events = client.tool_streamed(&app, "call", json!({ "operation": "delete_trade", "path": { "id": trade } })).await;
	let asked = events.next().await.unwrap();
	assert_eq!(client.answer(&app, &asked["id"], json!({ "action": "accept", "content": { "approve": true } })).await, StatusCode::ACCEPTED);
	let (deleted, error) = testing::tool_result(&events.next().await.unwrap());
	assert!(!error && deleted["status"] == 204, "{deleted}");
	let (trades, _) = client.tool(&app, "call", json!({ "operation": "trades" })).await;
	assert_eq!(trades["body"]["trades"].as_array().unwrap().len(), 0);
}
