//! The services' documents pulled into one catalog: under their forward prefixes, named after the
//! service, narrowed to the caller, and called through the forward as that caller.

use std::time::Duration;

use axum::{Json, Router, body::Body, http::Method, routing::get};
use sa_auth::Service;
use sa_panel::testing::{TestDb, panel};
use sa_server::{
	http,
	testing::{Browser, conductor, upstream},
};
use serde_json::{Value, json};
use tokio::sync::watch;

/// A service's document, as ev_lib's `api::document` writes one.
fn sites_document() -> Value {
	json!({
		"openapi": "3.1.0",
		"info": { "title": "sites", "version": "1" },
		"servers": [{ "url": "/api/v1" }],
		"paths": {
			"/leads/{id}": { "get": {
				"operationId": "lead",
				"summary": "A lead, whole.",
				"parameters": [{ "in": "path", "name": "id", "required": true, "schema": { "type": "integer" } }],
				"responses": { "200": { "description": "", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Lead" } } } } },
				"x-permission": "sa:sites:work:read"
			} },
			"/invoices": { "post": {
				"operationId": "create_invoice",
				"summary": "Invoices a lead.",
				"responses": { "201": { "description": "" } },
				"x-permission": "sa:sites:work:invoices:create"
			} }
		},
		"components": { "schemas": { "Lead": { "type": "object", "properties": { "id": { "type": "integer" } } } } }
	})
}

#[tokio::test]
async fn a_services_operations_join_the_catalog_under_its_prefix() {
	let db = TestDb::create().await;
	let p = panel(&db).await;
	let routes = Router::new()
		.route("/api/v1/openapi.json", get(|| async { Json(sites_document()) }))
		.route("/api/v1/leads/{id}", get(|| async { Json(json!({ "id": 7 })) }));
	let (base, seen) = upstream(routes).await;
	let operator_holds: sa_auth::PermissionSet = ["sa:sites:work:read"].into_iter().collect();
	let (sign_in, forward) = conductor(p.clone(), operator_holds, "op@localhost", &base);
	let catalog = forward.catalog().clone();
	let app = http::app(sign_in, Some(forward), None, Default::default(), None, None, None, None, None);
	let (stop, stopped) = watch::channel(false);
	let following = tokio::spawn({
		let catalog = catalog.clone();
		let services = sa_server::testing::services(&base);
		async move { catalog.follow(services, stopped).await }
	});
	tokio::time::timeout(Duration::from_secs(5), async {
		while catalog.document().paths.as_ref().unwrap().paths.get("/api/sites/leads/{id}").is_none() {
			tokio::time::sleep(Duration::from_millis(20)).await;
		}
	})
	.await
	.expect("sites' document pulled");
	stop.send(true).unwrap();
	following.await.unwrap();

	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (status, _, doc) = b.send_body(&app, Method::GET, "/api/v1/openapi.json", &[], false, Body::empty()).await;
	assert_eq!(status, 200);
	let doc: Value = serde_json::from_slice(&doc).unwrap();
	let lead = &doc["paths"]["/api/sites/leads/{id}"]["get"];
	assert_eq!(lead["operationId"], "sites.lead");
	assert_eq!(lead["responses"]["200"]["content"]["application/json"]["schema"]["$ref"], "#/components/schemas/sites.Lead");
	assert!(doc["components"]["schemas"]["sites.Lead"].is_object());
	assert!(doc["paths"]["/api/sites/invoices"].get("post").is_none(), "not the caller's to see");
	assert!(doc["paths"]["/api/v1/me"]["get"].is_object(), "the conductor's own, under /api/v1");

	let before = seen.lock().unwrap().len();
	let (status, _, lead) = b.send_body(&app, Method::GET, "/api/sites/leads/7", &[], false, Body::empty()).await;
	assert_eq!(status, 200, "{}", String::from_utf8_lossy(&lead));
	let seen = seen.lock().unwrap();
	let call = seen[before..].iter().find(|s| s.path_and_query == "/api/v1/leads/7").expect("forwarded");
	assert!(matches!(call.assertion(Service::Sites, None).principal, sa_auth::Principal::User(_)));
}
