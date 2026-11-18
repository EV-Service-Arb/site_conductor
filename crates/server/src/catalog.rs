//! One catalog of every operation behind the origin: the conductor's own `/api/v1` (its table,
//! `http::api_routes`) and each service's document, pulled as the dashboards are (`GET` under a
//! Conductor assertion, every 60 s, conditional; a failed pull reported, the last kept), its paths
//! under its forward prefix, its ids and schemas named `<service>.<name>`. What a caller is shown
//! is narrowed to their permissions — context, not authority: each service authorizes the call.

use std::{
	collections::BTreeMap,
	sync::{Arc, Mutex, RwLock},
	time::Duration,
};

use axum::{
	body::{Body, to_bytes},
	http::{HeaderMap, HeaderValue, Method, StatusCode, header},
};
use ev_lib::{
	api::{
		OpenApi,
		openapi::{Info, Operation, Paths, ReferenceOr, Server},
	},
	views::{Table, Views},
};
use eyre::WrapErr;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use sa_auth::{PermissionSet, Principal, Service};
use serde_json::{Map, Value};
use strum::VariantArray;
use tokio::sync::watch;

use crate::{
	forward::{self, Services},
	tools,
	view::View,
};

const PULL_EVERY: Duration = Duration::from_secs(60);
const PULL_TIMEOUT: Duration = Duration::from_secs(10);
const DOCUMENT_ANSWER: usize = 16 * 1024 * 1024;

/// What a path segment may hold as it is: RFC 3986's unreserved, but `.` (no value is a `..` segment).
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'~');
/// Each service's document and views, by name.
type Pulled = BTreeMap<String, (OpenApi, Option<Table>)>;

#[derive(Clone)]
pub struct Catalog {
	/// The conductor's own, under `/api/v1`.
	own: Arc<OpenApi>,
	/// Each service's, as last pulled, its prefix applied, and its views (`x-views`).
	services: Arc<RwLock<Pulled>>,
	/// `own` and `services` merged, absolute paths: rebuilt on a pull that changed something.
	merged: Arc<RwLock<Arc<OpenApi>>>,
	/// The conductor's views, each service's nested under its page: rebuilt as `merged` is.
	views: Arc<RwLock<Arc<Table>>>,
}

impl Catalog {
	pub(crate) fn new(own: OpenApi) -> Self {
		let own = Arc::new(own);
		let merged = Arc::new(RwLock::new(Arc::new(merge(&own, &BTreeMap::new()))));
		Self {
			own,
			services: Arc::default(),
			merged,
			views: Arc::new(RwLock::new(Arc::new(View::table().clone()))),
		}
	}

	/// Every view a person can be shown: the conductor's, the services' under their pages.
	pub(crate) fn views(&self) -> Arc<Table> {
		self.views.read().expect("never held across a panic").clone()
	}

	/// The conductor's own operations, relative to `/api/v1`: what the front end's client is
	/// generated from.
	pub fn own(&self) -> &OpenApi {
		&self.own
	}

	/// Every operation, its path absolute.
	pub fn document(&self) -> Arc<OpenApi> {
		self.merged.read().expect("never held across a panic").clone()
	}

	/// The method of the operation `id` and its absolute path, `path`'s parameters put in by name:
	/// refused for an unknown operation or a parameter missing.
	pub(crate) fn resolve(&self, id: &str, path: &Map<String, Value>) -> Result<(Method, String), String> {
		let doc = self.document();
		let (template, method, _) = doc
			.operations()
			.find(|(_, _, op)| op.operation_id.as_deref() == Some(id))
			.ok_or_else(|| format!("no operation {id:?}: search for one"))?;
		let mut uri = String::new();
		for segment in template.split('/').filter(|s| !s.is_empty()) {
			uri.push('/');
			match segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
				Some(name) => {
					let value = path.get(name).ok_or_else(|| format!("{id} needs the path parameter {name:?}"))?;
					uri.push_str(&utf8_percent_encode(&scalar(value), SEGMENT).to_string());
				}
				None => uri.push_str(segment),
			}
		}
		Ok((Method::from_bytes(method.to_uppercase().as_bytes()).expect("the document's methods are standard"), uri))
	}

	/// The operation `method` `path` is served by, with its path's parameters by name: of the
	/// templates that match, the one whose first differing segment is literal, as the routers pick.
	pub(crate) fn operation(&self, method: &Method, path: &str) -> Option<(Operation, Map<String, Value>)> {
		let doc = self.document();
		let asked: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
		let method = method.as_str().to_lowercase();
		doc.operations()
			.filter(|(_, m, _)| *m == method)
			.filter_map(|(template, _, op)| {
				let segments: Vec<&str> = template.split('/').filter(|s| !s.is_empty()).collect();
				if segments.len() != asked.len() {
					return None;
				}
				let mut params = Map::new();
				let mut literal = Vec::with_capacity(segments.len());
				for (segment, value) in segments.iter().zip(&asked) {
					match segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
						Some(name) => {
							params.insert(name.to_owned(), Value::String(percent_decode_str(value).decode_utf8().ok()?.into_owned())); // a value not UTF-8 matches no operation
							literal.push(false);
						}
						None if segment == value => literal.push(true),
						None => return None,
					}
				}
				Some((literal, op.clone(), params))
			})
			.max_by(|a, b| a.0.cmp(&b.0))
			.map(|(_, op, params)| (op, params))
	}

	/// What an agent is told of the panel before anything: the conductor's document's description,
	/// then each service's, as their documents say.
	pub(crate) fn instructions(&self) -> String {
		let services = self.services.read().expect("never held across a panic");
		std::iter::once(&*self.own)
			.chain(services.values().map(|(doc, _)| doc))
			.filter_map(|doc| doc.info.description.as_deref())
			.collect::<Vec<_>>()
			.join("\n\n")
	}

	/// Only the operations `permissions` open.
	pub(crate) fn opened(&self, permissions: &PermissionSet) -> OpenApi {
		let mut doc = (*self.document()).clone();
		for item in doc.paths.iter_mut().flat_map(|p| p.paths.values_mut()) {
			let ReferenceOr::Item(item) = item else { unreachable!("the document holds items only") };
			for op in [&mut item.get, &mut item.post, &mut item.put, &mut item.patch, &mut item.delete] {
				if op
					.as_ref()
					.and_then(|o| o.extensions.get("x-permission"))
					.is_some_and(|p| !permissions.iter().any(|held| p == held))
				{
					*op = None;
				}
			}
		}
		doc
	}

	/// Pulls each service's document until `stop`.
	pub async fn follow(&self, services: Services, stop: watch::Receiver<bool>) {
		let etags = Mutex::default();
		self.pull(&services, &etags).await;
		crate::every(PULL_EVERY, stop, "pulling the services' documents", || async {
			self.pull(&services, &etags).await;
			Ok(())
		})
		.await;
	}

	async fn pull(&self, services: &Services, etags: &Mutex<BTreeMap<String, HeaderValue>>) {
		let documents = Service::VARIANTS.iter().copied().filter(|s| services.serves(*s)).filter_map(|s| Some((s, s.document()?)));
		for (service, path) in documents {
			let name = service.to_string();
			match self.pull_one(services, service, path, etags).await {
				Ok(None) => {}
				Ok(Some(pulled)) => {
					let mut held = self.services.write().expect("never held across a panic");
					let mut next = held.clone();
					next.insert(name.clone(), pulled);
					let mut views = View::table().clone();
					let nested = next
						.iter()
						.filter_map(|(n, (_, v))| v.as_ref().map(|v| (n, v)))
						.try_for_each(|(n, v)| views.nest(n, v.clone()).map_err(|e| eyre::eyre!("{n}'s views: {e}")));
					match nested {
						Ok(()) => {
							*held = next;
							let docs: BTreeMap<String, OpenApi> = held.iter().map(|(n, (d, _))| (n.clone(), d.clone())).collect();
							*self.merged.write().expect("never held across a panic") = Arc::new(merge(&self.own, &docs));
							*self.views.write().expect("never held across a panic") = Arc::new(views);
						}
						Err(e) => crate::report(&e.wrap_err(format!("{name}'s operations kept as they were")), "pulling the services' documents"),
					}
				}
				Err(e) => crate::report(&e.wrap_err(format!("{name}'s operations kept as they were")), "pulling the services' documents"),
			}
		}
	}

	/// The service's document, prefixed and named; `None` when it has not changed.
	async fn pull_one(&self, services: &Services, service: Service, path: &str, etags: &Mutex<BTreeMap<String, HeaderValue>>) -> eyre::Result<Option<(OpenApi, Option<Table>)>> {
		let name = service.to_string();
		let mut headers = HeaderMap::new();
		if let Some(etag) = etags.lock().expect("held for a lookup only").get(&name) {
			headers.insert(header::IF_NONE_MATCH, etag.clone());
		}
		let call = services.send(service, Method::GET, path, headers, Some(Principal::Conductor), Body::empty());
		let res = tokio::time::timeout(PULL_TIMEOUT, call)
			.await
			.map_err(|_| eyre::eyre!("{name} did not answer GET {path} in time"))??;
		match res.status() {
			StatusCode::NOT_MODIFIED => return Ok(None),
			StatusCode::OK => {}
			s => eyre::bail!("{name} answered GET {path} with {s}"),
		}
		let etag = res.headers().get(header::ETAG).cloned();
		let body = to_bytes(res.into_body(), DOCUMENT_ANSWER).await.map_err(|e| eyre::eyre!("reading {name}'s document: {e}"))?;
		let mut doc: Value = serde_json::from_slice(&body).wrap_err_with(|| format!("{name}'s document"))?;
		let views = match doc.get_mut("x-views").map(Value::take) {
			Some(v) => Some(serde_json::from_value(v).wrap_err_with(|| format!("{name}'s x-views"))?),
			None => None,
		};
		let doc = (prefixed(&name, service, doc)?, views);
		if let Some(etag) = etag {
			etags.lock().expect("held for a lookup only").insert(name, etag);
		}
		Ok(Some(doc))
	}
}

/// The catalog as it stands once each of `vendored` is pulled: what the sandbox's client is generated from.
pub fn merged(vendored: impl IntoIterator<Item = (Service, Value)>) -> eyre::Result<OpenApi> {
	let docs = vendored
		.into_iter()
		.map(|(service, doc)| Ok((service.to_string(), prefixed(service.as_str(), service, doc)?)))
		.collect::<eyre::Result<_>>()?;
	Ok(merge(&crate::http::document(), &docs))
}

/// A service's document under its forward prefix, its ids and schemas named `<service>.<name>`.
fn prefixed(name: &str, service: Service, mut doc: Value) -> eyre::Result<OpenApi> {
	let base = doc["servers"][0]["url"].as_str().ok_or_else(|| eyre::eyre!("{name}'s document names no server"))?;
	let prefix = forward::prefix(service, base.trim_end_matches('/')).ok_or_else(|| eyre::eyre!("{name}'s document is served under {base}, which nothing forwards"))?;
	rename(&mut doc, name);
	if let Some(schemas) = doc["components"]["schemas"].as_object_mut() {
		*schemas = std::mem::take(schemas).into_iter().map(|(k, v)| (format!("{name}.{k}"), v)).collect();
	}
	let paths = doc["paths"].as_object_mut().ok_or_else(|| eyre::eyre!("{name}'s document has no paths"))?;
	*paths = std::mem::take(paths).into_iter().map(|(p, v)| (format!("{prefix}{p}"), v)).collect();
	for item in paths.values_mut().filter_map(Value::as_object_mut) {
		for op in item.values_mut().filter_map(Value::as_object_mut) {
			let id = op.get("operationId").and_then(Value::as_str).ok_or_else(|| eyre::eyre!("an operation of {name}'s has no id"))?;
			op.insert("operationId".into(), Value::String(format!("{name}.{id}")));
			op.insert("tags".into(), Value::Array(vec![Value::String(name.to_owned())]));
		}
	}
	// aide borrows the document's version string, which one read at runtime cannot lend: what the
	// catalog takes of it is read part by part.
	fn part<T: serde::de::DeserializeOwned>(doc: &mut Value, name: &str, key: &str) -> eyre::Result<T> {
		serde_json::from_value(doc[key].take()).wrap_err_with(|| format!("{name}'s document: its {key}"))
	}
	Ok(OpenApi {
		info: part(&mut doc, name, "info")?,
		paths: part(&mut doc, name, "paths")?,
		components: part(&mut doc, name, "components")?,
		..OpenApi::default()
	})
}

/// Every `$ref` to a schema of `name`'s.
fn rename(v: &mut Value, name: &str) {
	match v {
		Value::Object(o) => {
			if let Some(Value::String(r)) = o.get_mut("$ref")
				&& let Some(schema) = r.strip_prefix("#/components/schemas/")
			{
				*r = format!("#/components/schemas/{name}.{schema}");
			}
			o.values_mut().for_each(|v| rename(v, name));
		}
		Value::Array(a) => a.iter_mut().for_each(|v| rename(v, name)),
		_ => {}
	}
}

fn merge(own: &OpenApi, services: &BTreeMap<String, OpenApi>) -> OpenApi {
	let base = own.servers.first().expect("the conductor's document names its server").url.clone();
	let mut paths = Paths::default();
	let mut doc = own.clone();
	for (p, item) in own.paths.iter().flat_map(|p| p.paths.iter()) {
		paths.paths.insert(format!("{base}{p}"), item.clone());
	}
	let schemas = &mut doc.components.as_mut().expect("the conductor's document has components").schemas;
	for theirs in services.values() {
		paths.paths.extend(theirs.paths.iter().flat_map(|p| p.paths.clone()));
		schemas.extend(theirs.components.iter().flat_map(|c| c.schemas.clone()));
	}
	let mut tools = BTreeMap::new();
	let operations = paths.paths.values().flat_map(|item| {
		let ReferenceOr::Item(item) = item else { unreachable!("the document holds items only") };
		[&item.get, &item.post, &item.put, &item.patch, &item.delete].into_iter().flatten()
	});
	for op in operations.filter(|op| op.extensions.get("x-tool") == Some(&Value::Bool(true))) {
		let id = op.operation_id.as_deref().expect("every operation has an id");
		let name = tools::tool_name(id);
		assert!(!tools::BUILT_IN.contains(&name.as_str()), "{id} would be the agents' own tool {name}");
		if let Some(other) = tools.insert(name.clone(), id) {
			panic!("{id} and {other} would both be the tool {name}");
		}
	}
	OpenApi {
		info: Info {
			title: "Service-Arb".into(),
			..own.info.clone()
		},
		servers: vec![Server {
			url: "/".into(),
			..Server::default()
		}],
		paths: Some(paths),
		..doc
	}
}

/// A string as it is, anything else as JSON writes it.
pub(crate) fn scalar(v: &Value) -> String {
	v.as_str().map_or_else(|| v.to_string(), str::to_owned)
}
