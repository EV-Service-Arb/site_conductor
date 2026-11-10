//! A gmail's antidetect browser: one fingerprint and one session per gmail, at any of its GBPs.
//! Its proxy is checked before it is kept (one request through it to ip-api: the exit, which
//! must be in the country of every GBP of the gmail), sealed, and no other gmail's: proxies
//! compare by a keyed tag, never in the clear. The provider's profile is made at the first start,
//! its noise drawn by the provider then and kept; what a start hands sa_browsers is a
//! [`Launch`].
//!
//! ```text
//! set:   proxy ── public address ── ip-api through it ── exit in every GBP's country ── sealed + tag
//! start: GoLogin profile (made once, its noise kept) ── PUT the fingerprint ── 3-minute token
//!        ── Launch { Orbita prefs: device, zone and locale of the GBP's point (else the exit's),
//!           the position that point or blocked }
//! ```

use std::{sync::LazyLock, time::Duration};

use eyre::WrapErr;
use icu_locale::{LanguageIdentifier, LocaleExpander};
use jiff::Timestamp;
use sa_browsers::{Device, Launch};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use zeroize::Zeroizing;

pub use crate::store::browsers::{Browser, Exit, Provider};
use crate::{
	Panel, Refused, imap,
	seal::gmail_browser_aad,
	store::{
		browsers::{self as db, Noise, Proxy},
		gmbs as gmbs_db,
	},
	tokens::is_open,
};

const CHECK_WITHIN: Duration = Duration::from_secs(15);
/// An integer: Orbita reads it as one, and a JSON double there crashes its renderer.
const GPS_ACCURACY_M: u32 = 10;
const PLATFORM: &str = "Linux armv81";

static ZONES: LazyLock<tzf_rs::DefaultFinder> = LazyLock::new(tzf_rs::DefaultFinder::new);

/// What a browser needs beyond the database: the provider, and the exit's lookup.
pub struct Browsers {
	gologin: GoLogin,
	exits: Exits,
}

impl Browsers {
	pub fn new(gologin_token: Zeroizing<String>) -> eyre::Result<Self> {
		Ok(Self {
			gologin: GoLogin::new("https://api.gologin.com", gologin_token)?,
			exits: Exits {
				lookup: "http://ip-api.com/json/".to_owned(),
				public_only: true,
			},
		})
	}

	/// GoLogin's API and ip-api at stubs', a proxy on loopback let through.
	#[cfg(feature = "testing")]
	pub fn stubbed(gologin: &str, ip_api: &str) -> Self {
		Self {
			gologin: GoLogin::new(gologin, Zeroizing::new("stub".to_owned())).expect("a client"),
			exits: Exits {
				lookup: ip_api.to_owned(),
				public_only: false,
			},
		}
	}
}

/// A browser as its owner sets it up.
pub struct NewBrowser {
	pub provider: Provider,
	pub device: Device,
	pub proxy: Zeroizing<String>,
}

impl Panel {
	/// The gmail's browser, stored once its proxy goes through, out in its GBPs' country, and is
	/// no other gmail's; a refusal stores nothing. Provider and device are set once: the proxy
	/// alone is replaced.
	pub async fn set_browser(&self, b: &Browsers, owner: Uuid, id: i64, new: NewBrowser, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let proxy = match ProxyUrl::parse(&new.proxy) {
			Ok(p) => p,
			Err(why) => return Ok(Err(Refused::Invalid(why))),
		};
		let tag = self.key.proxy_tag(&proxy.normalized());
		if let Err(r) = self.gmail_of(owner, id).await? {
			return Ok(Err(r));
		}
		{
			let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a gmail's browser")?;
			if let Some((set, _)) = db::browser(&mut conn, id).await?
				&& (set.provider, set.device) != (new.provider, new.device)
			{
				return Ok(Err(Refused::Conflict(format!(
					"this gmail's browser is {} as a {}: a fingerprint is set once; remove it to start another",
					set.provider,
					<&str>::from(set.device)
				))));
			}
			if db::tag_holder(&mut conn, &tag).await?.is_some_and(|holder| holder != id) {
				return Ok(Err(Refused::Invalid("another gmail leaves through this proxy: one proxy is one gmail's".to_owned())));
			}
		}
		let exit = match b.exits.check(&proxy).await? {
			Ok(e) => e,
			Err(why) => return Ok(Err(Refused::Invalid(why))),
		};
		let mut tx = self.store.begin_write().await?;
		if let Err(why) = in_countries(&mut tx, id, &exit).await? {
			return Ok(Err(Refused::Invalid(why)));
		}
		if gmbs_db::gmail_owner(&mut tx, id).await? != Some(owner) {
			return Ok(Err(Refused::NotFound));
		}
		let stored = Proxy {
			sealed: self.seal(&gmail_browser_aad(id), &new.proxy)?,
			tag,
			endpoint: proxy.endpoint(),
		};
		db::set(&mut tx, id, new.provider, new.device, &stored, &exit, now).await?;
		tx.commit().await.wrap_err("committing a gmail's browser")?;
		Ok(Ok(()))
	}

	/// The exit checked again through the stored proxy, and what it said recorded.
	pub async fn check_browser(&self, b: &Browsers, owner: Uuid, id: i64, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		if let Err(r) = self.gmail_of(owner, id).await? {
			return Ok(Err(r));
		}
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a gmail's browser")?;
		let Some((_, sealed)) = db::browser(&mut conn, id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		drop(conn);
		let proxy = ProxyUrl::parse(&self.unseal(&gmail_browser_aad(id), &sealed)?).map_err(|why| eyre::eyre!("a stored proxy no longer parses: {why}"))?;
		let seen = b.exits.check(&proxy).await?;
		let mut tx = self.store.begin_write().await?;
		let seen = match seen {
			Ok(exit) => in_countries(&mut tx, id, &exit).await?.map(|()| exit),
			Err(why) => Err(why),
		};
		db::checked(&mut tx, id, now, seen.as_ref().map_err(String::as_str)).await?;
		tx.commit().await.wrap_err("committing a browser's check")?;
		Ok(Ok(()))
	}

	/// The gmail `id` is `owner`'s and has a browser.
	pub async fn browser_of(&self, owner: Uuid, id: i64) -> eyre::Result<Result<Browser, Refused>> {
		if let Err(r) = self.gmail_of(owner, id).await? {
			return Ok(Err(r));
		}
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a gmail's browser")?;
		Ok(db::browser(&mut conn, id).await?.map(|(b, _)| b).ok_or(Refused::NotFound))
	}

	/// The provider's profile and the row: once its runner let it go.
	pub async fn delete_browser(&self, b: &Browsers, owner: Uuid, id: i64) -> eyre::Result<Result<(), Refused>> {
		let browser = match self.browser_of(owner, id).await? {
			Ok(b) => b,
			Err(r) => return Ok(Err(r)),
		};
		if let Some((profile, _)) = &browser.profile
			&& let Err(why) = b.gologin.delete(profile).await?
		{
			return Ok(Err(Refused::Unavailable(why)));
		}
		let mut tx = self.store.begin_write().await?;
		db::delete(&mut tx, id).await?;
		tx.commit().await.wrap_err("committing a browser's removal")?;
		Ok(Ok(()))
	}

	/// What sa_browsers starts the gmail's browser with: at `gmb`'s point, else with the position
	/// blocked; the provider's profile made the first time.
	pub async fn launch(&self, b: &Browsers, owner: Uuid, id: i64, gmb: Option<i64>, now: Timestamp) -> eyre::Result<Result<Launch, Refused>> {
		let (gmail, browser, sealed, gmb) = {
			let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a launch")?;
			if !is_open(&mut conn).await? {
				return Ok(Err(Refused::Closed));
			}
			let Some(gmail) = gmbs_db::gmails(&mut conn, owner).await?.into_iter().find(|g| g.id == id) else {
				return Ok(Err(Refused::NotFound));
			};
			let Some((browser, sealed)) = db::browser(&mut conn, id).await? else {
				return Ok(Err(Refused::Conflict("this gmail has no browser yet: set one up in its connections".to_owned())));
			};
			let gmb = match gmb {
				Some(g) => match gmbs_db::gmb_of(&mut conn, Some(owner), g).await? {
					Some(gmb) if gmb.gmail_id == Some(id) => Some(gmb),
					_ => return Ok(Err(Refused::NotFound)),
				},
				None => None,
			};
			(gmail, browser, sealed, gmb)
		};
		if let Some(why) = &browser.checked.error {
			return Ok(Err(Refused::Conflict(format!("its proxy's last check failed ({why}): test it again"))));
		}
		let place = match &gmb {
			Some(g) => {
				let Some(point) = &g.location else {
					return Ok(Err(Refused::Conflict(format!("{} has no point yet: place it on the map", g.name))));
				};
				let Some(country) = &g.country else {
					return Ok(Err(Refused::Conflict(format!("{} has no country yet", g.name))));
				};
				if country.code.as_str() != browser.exit.country {
					return Ok(Err(Refused::Conflict(format!(
						"{} is in {}, and the proxy comes out in {}",
						g.name, country.code, browser.exit.country
					))));
				}
				let zone = ZONES.get_tz_name(point.at.lng, point.at.lat).to_owned();
				Place::new(zone, &browser.exit.country, Some((point.at.lat, point.at.lng)))
			}
			None => Place::new(browser.exit.zone.clone(), &browser.exit.country, None),
		};
		let place = match place {
			Ok(p) => p,
			Err(why) => return Ok(Err(Refused::Conflict(why))),
		};
		let proxy = ProxyUrl::parse(&self.unseal(&gmail_browser_aad(id), &sealed)?).map_err(|why| eyre::eyre!("a stored proxy no longer parses: {why}"))?;
		let hw = browser.device.hardware();
		let profile = match browser.profile {
			Some(p) => p,
			None => {
				let made = match b.gologin.create(&fingerprint(&gmail.gmail, browser.device, &place, &proxy, None)).await? {
					Ok(id) => id,
					Err(why) => return Ok(Err(Refused::Unavailable(why))),
				};
				let noise = match b.gologin.noise(&made).await? {
					Ok(n) => n,
					Err(why) => return Ok(Err(Refused::Unavailable(why))),
				};
				let mut tx = self.store.begin_write().await?;
				db::set_profile(&mut tx, id, &made, &noise).await?;
				tx.commit().await.wrap_err("committing a browser's provider profile")?;
				tracing::info!(gmail_id = id, profile = made, "GoLogin profile made");
				(made, noise)
			}
		};
		let (profile, noise) = profile;
		if let Err(why) = b.gologin.put(&profile, &fingerprint(&gmail.gmail, browser.device, &place, &proxy, Some(&noise))).await? {
			return Ok(Err(Refused::Unavailable(why)));
		}
		let mut prefs = json!({
			"profile_id": profile,
			"name": gmail.gmail,
			"userAgent": Device::user_agent(),
			"platform_version": hw.android,
			"navigator": { "platform": PLATFORM, "max_touch_points": hw.touch },
			"screenWidth": hw.screen.0,
			"screenHeight": hw.screen.1,
			"mobile": { "enable": true, "width": hw.screen.0, "height": hw.screen.1, "device_scale_factor": hw.dpr },
			"hardwareConcurrency": hw.cores,
			"deviceMemory": hw.memory * 1024,
			"webGl": { "vendor": hw.gpu_vendor, "renderer": hw.gpu_renderer, "mode": true },
			"canvas": { "mode": "noise" },
			"storage": { "enable": true },
			"languages": place.languages().join(","),
			"langHeader": place.accept_language(),
			"timezone": { "id": place.zone },
			"geoLocation": match place.fix {
				Some((latitude, longitude)) => json!({ "mode": "prompt", "latitude": latitude, "longitude": longitude, "accuracy": GPS_ACCURACY_M }),
				None => json!({ "mode": "block" }),
			},
			"proxy": { "mode": proxy.scheme, "username": proxy.username, "password": *proxy.password },
		});
		let preferences = json!({
			"gologin": prefs.clone(),
			// a proxy's UDP would go around it from the runner's own address
			"webrtc": { "ip_handling_policy": "disable_non_proxied_udp" },
		});
		let token = match b.gologin.token(&profile).await? {
			Ok(t) => t,
			Err(why) => return Ok(Err(Refused::Unavailable(why))),
		};
		prefs["api_domain"] = b.gologin.base.clone().into();
		prefs["profile_token"] = token.as_str().into();
		let languages = place.languages().join(",");
		let Some(app_locale) = sa_browsers::ui_locale(&place.locale) else {
			return Ok(Err(Refused::Conflict(format!("Orbita has no UI locale for {}", place.locale))));
		};
		let config = json!({
			"intl": { "accept_languages": languages, "selected_languages": languages, "app_locale": app_locale, "forced_languages": [place.locale] },
			"gologin": prefs,
		});
		tracing::info!(gmail_id = id, gmb_id = ?gmb.as_ref().map(|g| g.id), zone = place.zone, locale = place.locale, %now, "browser launch prepared");
		Ok(Ok(Launch {
			run: gmb.map_or_else(|| "the gmail alone".to_owned(), |g| g.name),
			browser: sa_browsers::Browser::Orbita {
				preferences,
				config,
				proxy: format!("{}://{}:{}", proxy.scheme, proxy.host, proxy.port),
				zone: place.zone,
				locale: place.locale,
				screen: hw.screen,
			},
		}))
	}
}

/// `exit` in the country of every GBP of the gmail that has one.
async fn in_countries(conn: &mut sqlx::SqliteConnection, gmail_id: i64, exit: &Exit) -> eyre::Result<Result<(), String>> {
	let elsewhere: Vec<String> = db::gbp_countries(conn, gmail_id).await?.into_iter().filter(|c| *c != exit.country).collect();
	Ok(match elsewhere.is_empty() {
		true => Ok(()),
		false => Err(format!("the proxy comes out in {}, and the gmail's GBPs are in {}", exit.country, elsewhere.join(", "))),
	})
}

/// Where the browser says it is.
struct Place {
	zone: String,
	locale: String,
	/// Latitude, longitude; `None`, the position blocked.
	fix: Option<(f64, f64)>,
}

impl Place {
	fn new(zone: String, country: &str, fix: Option<(f64, f64)>) -> Result<Self, String> {
		if zone.is_empty() || zone.starts_with("Etc/") {
			return Err(format!("no land zone there ('{zone}'), and a phone is always in one"));
		}
		let mut lang: LanguageIdentifier = format!("und-{country}").parse().map_err(|e| format!("country {country}: {e}"))?;
		LocaleExpander::new_extended().maximize(&mut lang);
		if lang.language.is_unknown() {
			return Err(format!("no likely language for {country}"));
		}
		Ok(Self {
			zone,
			locale: format!("{}-{country}", lang.language),
			fix,
		})
	}

	fn languages(&self) -> Vec<String> {
		let lang = self.locale.split('-').next().expect("split yields at least one").to_owned();
		let mut v = vec![self.locale.clone(), lang, "en-US".into(), "en".into()];
		let mut seen = std::collections::HashSet::new();
		v.retain(|l| seen.insert(l.clone()));
		v
	}

	fn accept_language(&self) -> String {
		self.languages()
			.iter()
			.enumerate()
			.map(|(i, l)| if i == 0 { l.clone() } else { format!("{l};q=0.{}", 10 - i) })
			.collect::<Vec<_>>()
			.join(",")
	}
}

/// What GoLogin's dashboard shows, not only what Orbita reads from it, so the profile there is
/// never a second, stale opinion of the gmail. `noise` `None` at creation: GoLogin draws it.
fn fingerprint(name: &str, device: Device, place: &Place, proxy: &ProxyUrl, noise: Option<&Noise>) -> Value {
	let hw = device.hardware();
	let mut v = json!({
		"name": name,
		"os": "android",
		"devicePixelRatio": hw.dpr,
		"navigator": {
			"userAgent": Device::user_agent(),
			"resolution": format!("{}x{}", hw.screen.0, hw.screen.1),
			"language": place.accept_language(),
			"platform": PLATFORM,
			"hardwareConcurrency": hw.cores,
			"deviceMemory": hw.memory,
			"maxTouchPoints": hw.touch,
		},
		"proxy": { "mode": proxy.scheme, "host": proxy.host, "port": proxy.port, "username": proxy.username, "password": *proxy.password },
		"webGLMetadata": { "mode": "mask", "vendor": hw.gpu_vendor, "renderer": hw.gpu_renderer },
	});
	if let Some(n) = noise {
		v["webGL"] = json!({ "mode": "noise", "noise": n.webgl, "getClientRectsNoise": n.rects });
		v["clientRects"] = json!({ "mode": "noise", "noise": n.rects });
		v["canvas"] = json!({ "mode": "noise", "noise": n.canvas });
		v["audioContext"] = json!({ "mode": "noise", "noise": n.audio });
	}
	v
}

/// A proxy as pasted: `socks5://` or `http://`, `user:pass@host:port`.
struct ProxyUrl {
	scheme: &'static str,
	host: String,
	port: u16,
	username: String,
	password: Zeroizing<String>,
}

impl ProxyUrl {
	fn parse(raw: &str) -> Result<Self, String> {
		let bad = || "a proxy is socks5:// or http://, then user:pass@host:port".to_owned();
		let url = url::Url::parse(raw.trim()).map_err(|_| bad())?;
		let scheme = match url.scheme() {
			"socks5" | "socks5h" => "socks5",
			"http" => "http",
			_ => return Err(bad()),
		};
		let host = url.host_str().filter(|h| !h.is_empty()).ok_or_else(bad)?.to_lowercase();
		let port = url.port().ok_or_else(bad)?;
		let decode = |s: &str| percent_encoding::percent_decode_str(s).decode_utf8().map(|c| c.into_owned()).map_err(|_| bad());
		let username = decode(url.username())?;
		let password = Zeroizing::new(decode(url.password().ok_or_else(bad)?)?);
		if username.is_empty() || password.is_empty() || !(url.path().is_empty() || url.path() == "/") || url.query().is_some() {
			return Err(bad());
		}
		Ok(Self {
			scheme,
			host,
			port,
			username,
			password,
		})
	}

	/// What the tag is of: one proxy however it was spelled.
	fn normalized(&self) -> String {
		format!("{}://{}:{}@{}:{}", self.scheme, self.username, *self.password, self.host, self.port)
	}

	fn endpoint(&self) -> String {
		format!("{}:{}", self.host, self.port)
	}
}

/// Where a proxy comes out: ip-api, asked through it.
struct Exits {
	lookup: String,
	/// The proxy's host resolves to public addresses only: a pasted host never points the
	/// conductor, nor the runner after it, at an inner service.
	public_only: bool,
}

impl Exits {
	/// The exit, or why the proxy did not take the conductor there; `Err` is ours.
	async fn check(&self, proxy: &ProxyUrl) -> eyre::Result<Result<Exit, String>> {
		let addrs: Vec<std::net::SocketAddr> = match tokio::net::lookup_host((proxy.host.as_str(), proxy.port)).await {
			Ok(a) => a.collect(),
			Err(e) => return Ok(Err(format!("{} does not resolve: {e}", proxy.host))),
		};
		let Some(addr) = addrs.first() else {
			return Ok(Err(format!("{} resolves to no address", proxy.host)));
		};
		if self.public_only
			&& let Some(inner) = addrs.iter().find(|a| !imap::public(a.ip()))
		{
			return Ok(Err(format!("{} resolves to {}, not a public address", proxy.host, inner.ip())));
		}
		// pinned to the address checked; names past it resolve at the proxy, as the browser's do
		let scheme = if proxy.scheme == "socks5" { "socks5h" } else { "http" };
		let pinned = reqwest::Proxy::all(format!("{scheme}://{addr}"))
			.wrap_err("a proxy for the check")?
			.basic_auth(&proxy.username, &proxy.password);
		let client = reqwest::Client::builder().proxy(pinned).timeout(CHECK_WITHIN).build().wrap_err("a client for the check")?;
		#[derive(Deserialize)]
		#[serde(rename_all = "camelCase")]
		struct IpApi {
			status: String,
			message: Option<String>,
			country_code: Option<String>,
			city: Option<String>,
			timezone: Option<String>,
			mobile: Option<bool>,
			query: Option<String>,
		}
		let asked = client.get(format!("{}?fields=status,message,countryCode,city,timezone,mobile,query", self.lookup)).send().await;
		let answer = match asked {
			Ok(r) if r.status().is_success() => r.json::<IpApi>().await,
			Ok(r) => return Ok(Err(format!("through the proxy, ip-api answered {}", r.status()))),
			Err(e) => return Ok(Err(format!("the proxy did not take the request: {e}"))),
		};
		let a = match answer {
			Ok(a) => a,
			Err(e) => return Ok(Err(format!("through the proxy, ip-api's answer did not read: {e}"))),
		};
		if a.status != "success" {
			return Ok(Err(format!("ip-api: {}", a.message.unwrap_or(a.status))));
		}
		let (Some(ip), Some(country), Some(city), Some(zone), Some(mobile)) = (a.query, a.country_code, a.city, a.timezone, a.mobile) else {
			return Ok(Err("ip-api's answer lacks the exit's address, country, city, zone or carrier".to_owned()));
		};
		Ok(Ok(Exit { ip, country, city, zone, mobile }))
	}
}

/// GoLogin's API: the profiles Orbita reads its server-side fingerprint from.
struct GoLogin {
	base: String,
	http: reqwest::Client,
}

impl GoLogin {
	fn new(base: &str, token: Zeroizing<String>) -> eyre::Result<Self> {
		let mut auth = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", *token)).wrap_err("GOLOGIN_TOKEN is not a header value")?;
		auth.set_sensitive(true);
		let headers = reqwest::header::HeaderMap::from_iter([(reqwest::header::AUTHORIZATION, auth)]);
		Ok(Self {
			base: base.to_owned(),
			// api.gologin.com's edge refuses a request without a user agent
			http: reqwest::Client::builder()
				.default_headers(headers)
				.user_agent("sa-conductor")
				.timeout(Duration::from_secs(30))
				.build()
				.wrap_err("GoLogin's client")?,
		})
	}

	/// The answer's JSON, or what GoLogin said, for the operator; `Err` is ours.
	async fn call(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> eyre::Result<Result<Value, String>> {
		let req = self.http.request(method.clone(), format!("{}{path}", self.base));
		let req = match body {
			Some(b) => req.json(b),
			None => req,
		};
		let res = match req.send().await {
			Ok(r) => r,
			Err(e) => return Ok(Err(format!("GoLogin did not answer {method} {path}: {e}"))),
		};
		let status = res.status();
		let text = res.text().await.wrap_err_with(|| format!("reading GoLogin's answer to {method} {path}"))?;
		if !status.is_success() {
			return Ok(Err(format!("GoLogin refused {method} {path}: {status} {}", text.chars().take(300).collect::<String>())));
		}
		// some of its answers are a bare sentence ("Profile updated successfully")
		Ok(Ok(serde_json::from_str(&text).unwrap_or(Value::Null)))
	}

	async fn create(&self, body: &Value) -> eyre::Result<Result<String, String>> {
		Ok(self
			.call(reqwest::Method::POST, "/browser/custom", Some(body))
			.await?
			.and_then(|v| v["id"].as_str().map(str::to_owned).ok_or_else(|| format!("GoLogin made a profile and named none: {v}"))))
	}

	/// What GoLogin drew at the profile's creation.
	async fn noise(&self, id: &str) -> eyre::Result<Result<Noise, String>> {
		Ok(self.call(reqwest::Method::GET, &format!("/browser/{id}"), None).await?.and_then(|v| {
			let n = |section: &str| v[section]["noise"].as_f64().ok_or_else(|| format!("GoLogin's profile {id} has no {section} noise"));
			Ok(Noise {
				canvas: n("canvas")?,
				webgl: n("webGL")?,
				rects: n("clientRects")?,
				audio: n("audioContext")?,
			})
		}))
	}

	async fn put(&self, id: &str, body: &Value) -> eyre::Result<Result<(), String>> {
		Ok(self.call(reqwest::Method::PUT, &format!("/browser/{id}/custom"), Some(body)).await?.map(drop))
	}

	/// Alive three minutes; Orbita reads it once, at start.
	async fn token(&self, id: &str) -> eyre::Result<Result<Zeroizing<String>, String>> {
		Ok(self
			.call(reqwest::Method::GET, &format!("/browser/features/{id}/profile-params-for-orbita-token"), None)
			.await?
			.and_then(|v| v["token"].as_str().map(|t| Zeroizing::new(t.to_owned())).ok_or_else(|| "GoLogin gave no Orbita token".to_owned())))
	}

	async fn delete(&self, id: &str) -> eyre::Result<Result<(), String>> {
		Ok(self.call(reqwest::Method::DELETE, &format!("/browser/{id}"), None).await?.map(drop))
	}
}
