//! The front end: the Next static export (`frontend/`, `npm run build` → `out/`), served from
//! `PANEL_WEB_DIR` on the same origin as `/api` and `/auth`, behind every route of its own.
//!
//! ```text
//! /api, /auth, /health, /grafana,  never the front end: a route of theirs, else a JSON 404
//! /cabinet, the forward's prefixes
//! a view (`View::parse` takes it) the one page, index.html
//! a file                           itself
//! anything else                    404.html, status 404
//! ```
//!
//! Files are read by tower-http's `ServeDir`, which refuses any path that would leave the
//! directory (`..`, a drive or a root component, encoded or not) before touching the disk.
//!
//! Headers: `/_next/static/*` is content-hashed, so `immutable` for a year; everything else
//! `no-cache`, and every HTML page a CSP.

use std::{io, path::Path};

use axum::{
	Router,
	body::Body,
	extract::Request,
	http::{HeaderValue, StatusCode, Uri, header},
	response::{IntoResponse, Response},
};
use ev_lib::{api::Refusal, views::Views};
use tower_http::{
	services::{ServeDir, ServeFile},
	set_status::SetStatus,
};

use crate::{api::Code, view::View};

/// The pages' policy: never framed, no `<base>`, forms posted only here. What a page loads is
/// left to the browser's defaults: Maps JS, the platform's elements and Turnstile come from
/// their own origins.
const CSP: &str = "frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

/// Where the build puts what it names by content hash.
const HASHED: &str = "/_next/static/";

/// The export's files.
#[derive(Clone, Debug)]
pub struct Files {
	dir: ServeDir<SetStatus<ServeFile>>,
}

impl Files {
	/// The export in `dir`; it must hold `index.html` and `404.html`, so a wrong path fails the
	/// boot rather than every page.
	pub fn try_new(dir: &Path) -> eyre::Result<Self> {
		for page in ["index.html", "404.html"] {
			let path = dir.join(page);
			eyre::ensure!(path.is_file(), "PANEL_WEB_DIR {} has no {page}: not a build of frontend/", dir.display());
		}
		let dir = ServeDir::new(dir).append_index_html_on_directories(true).not_found_service(ServeFile::new(dir.join("404.html")));
		Ok(Self { dir })
	}
}

/// `app`, with the front end answering whatever none of its routes does.
pub fn serve(app: Router, files: Files) -> Router {
	app.fallback(move |req: Request| page(files.clone(), req))
}

/// The origin's prefixes are the API side's, whether or not a route of theirs is mounted: an
/// unknown path below one is a JSON 404, never the front end's page.
fn reserved(path: &str) -> bool {
	crate::forward::paths().any(|p| p.covers(path))
}

async fn page(mut files: Files, mut req: Request) -> Response {
	if reserved(req.uri().path()) {
		return dressed(Refusal::new(Code::NotFound, "not found").into_response(), false);
	}
	let asked = req.uri().path_and_query().map_or(req.uri().path(), |pq| pq.as_str());
	if View::parse(asked).is_ok() {
		*req.uri_mut() = Uri::from_static("/");
	}
	let hashed = req.uri().path().starts_with(HASHED);
	// Every file in the image has the Nix store's mtime, 1970: answering If-Modified-Since
	// from it would keep a browser on the previous release's HTML, whose chunks are gone.
	// So nothing is revalidated by date — the HTML is small, and the rest is hashed.
	for h in [header::IF_MODIFIED_SINCE, header::IF_UNMODIFIED_SINCE] {
		req.headers_mut().remove(h);
	}
	match files.dir.try_call(req).await {
		Ok(res) => dressed(res.map(Body::new), hashed),
		Err(e) => failed(&e),
	}
}

fn failed(e: &io::Error) -> Response {
	crate::report(&eyre::eyre!("{e}"), "serving the front end");
	dressed(StatusCode::INTERNAL_SERVER_ERROR.into_response(), false)
}

fn dressed(mut res: Response, hashed: bool) -> Response {
	let ok = res.status().is_success();
	let h = res.headers_mut();
	h.remove(header::LAST_MODIFIED);
	h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
	// the origin, never the path, to other origins: Maps JS checks its referrer-restricted key by it
	h.insert(header::REFERRER_POLICY, HeaderValue::from_static("strict-origin-when-cross-origin"));
	let html = h.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("text/html"));
	let cache = if hashed && ok && !html { "public, max-age=31536000, immutable" } else { "no-cache" };
	h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
	if html {
		h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
	}
	res
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn reserved_prefixes_are_whole_segments() {
		for p in [
			"/api", "/api/", "/api/v1/nope", "/auth/x", "/health", "/grafana", "/grafana/d/1", "/cabinet", "/cabinet/mfe/x.js", "/mfe/banking/x.js",
		] {
			assert!(reserved(p), "{p}");
		}
		for p in [
			"/",
			"/apiary/",
			"/authors",
			"/healthy",
			"/cabinets",
			"/leads/",
			"/_next/static/a.js",
			"/reviews/archive",
			"/reviews/archive/gmails/3",
			"/sites/leads",
		] {
			assert!(!reserved(p), "{p}");
		}
		for p in ["/mfe/review_archive/x.js", "/mfe/sites/mfe-sites.js", "/api/sites/leads", "/api/playbook/search"] {
			assert!(reserved(p), "{p}");
		}
	}
}
