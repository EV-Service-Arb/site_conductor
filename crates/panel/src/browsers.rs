//! A gmail's antidetect browser: one fingerprint and one session per gmail, at any of its GBPs.
//! Its proxy is checked before it is kept (one request through it to ip-api: the exit, which
//! must be in the country of every GBP of the gmail), sealed, and no other gmail's: proxies
//! compare by a keyed tag, never in the clear. What a start hands sa_browsers is a [`Launch`].
//!
//! ```text
//! set:   proxy ── public address ── ip-api through it ── exit in every GBP's country ── sealed + tag
//!        Multilogin's phone (its model drawn) or Mimic profile made now, its proxy replaced after
//! start: GoLogin: profile (made once, its noise kept) ── PUT the fingerprint ── 3-minute token
//!        Multilogin: Mimic's profile told the place; a 3-hour run token
//!        ── Launch at the GBP's point (zone and locale its), else the exit's zone, no position
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

pub use crate::store::browsers::{Browser, Exit, Poses, Provider};
use crate::{
	Panel, Refused, imap,
	seal::gmail_browser_aad,
	store::{
		browsers::{self as db, Fingerprint, Noise, Proxy},
		gmbs as gmbs_db,
	},
	tokens::is_open,
};

/// A phone's film in Multilogin's Library, at most.
pub const LIBRARY_MAX: usize = 100 << 20;
const CHECK_WITHIN: Duration = Duration::from_secs(15);
/// An integer: Orbita reads it as one, and a JSON double there crashes its renderer.
const GPS_ACCURACY_M: u32 = 10;
const PLATFORM: &str = "Linux armv81";
/// 14 or 16 pass Play Integrity without a keybox of one's own (Multilogin's docs).
const ANDROID: u32 = 14;
/// Where the gmails' Mimic profiles sit in Multilogin.
const FOLDER: &str = "site_conductor";

/// What a browser needs beyond the database: the providers connected, and the exit's lookup.
pub struct Browsers {
	gologin: Option<GoLogin>,
	multilogin: Option<Multilogin>,
	exits: Exits,
}
impl Browsers {
	pub fn try_new(gologin_token: Option<Zeroizing<String>>, multilogin_token: Option<Zeroizing<String>>) -> eyre::Result<Self> {
		Ok(Self {
			gologin: gologin_token.map(|t| GoLogin::try_new("https://api.gologin.com", t)).transpose()?,
			multilogin: multilogin_token.map(|t| Multilogin::try_new("https://api.multilogin.com", t)).transpose()?,
			exits: Exits {
				lookup: "http://ip-api.com/json/".to_owned(),
				public_only: true,
			},
		})
	}

	/// The providers' APIs and ip-api at stubs', a proxy on loopback let through.
	#[cfg(feature = "testing")]
	pub fn stubbed(gologin: &str, multilogin: &str, ip_api: &str) -> Self {
		Self {
			gologin: Some(GoLogin::try_new(gologin, Zeroizing::new("stub".to_owned())).expect("a client")),
			multilogin: Some(Multilogin::try_new(multilogin, Zeroizing::new("stub".to_owned())).expect("a client")),
			exits: Exits {
				lookup: ip_api.to_owned(),
				public_only: false,
			},
		}
	}

	/// Once [`Self::of`] said its provider is connected.
	fn gl(&self) -> &GoLogin {
		self.gologin.as_ref().expect("its provider checked connected")
	}

	fn ml(&self) -> &Multilogin {
		self.multilogin.as_ref().expect("its provider checked connected")
	}

	/// The provider's token was given.
	fn of(&self, provider: Provider) -> Result<(), Refused> {
		let connected = match provider {
			Provider::Gologin => self.gologin.is_some(),
			Provider::Multilogin | Provider::MultiloginBrowser => self.multilogin.is_some(),
		};
		connected.then_some(()).ok_or_else(|| Refused::Unavailable(format!("{provider} not connected")))
	}
}

/// A browser as its owner sets it up.
pub struct NewBrowser {
	pub provider: Provider,
	/// A browser's; a cloud phone's model is drawn.
	pub device: Option<Device>,
	pub proxy: Zeroizing<String>,
}

/// What a run is for, as the runner keys it: a GBP's point, or the gmail alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Run(pub Option<i64>);

static ZONES: LazyLock<tzf_rs::DefaultFinder> = LazyLock::new(tzf_rs::DefaultFinder::new);

/// `?` for a provider's refusal: what it said, as the owner is told it.
macro_rules! said {
	($e:expr) => {
		match $e {
			Ok(v) => v,
			Err(why) => return Ok(Err(Refused::Unavailable(why))),
		}
	};
}

impl Panel {
	/// The gmail's browser, stored once its proxy goes through, out in its GBPs' country, and is
	/// no other gmail's; a refusal stores nothing. Provider and device are set once: the proxy
	/// alone is replaced. Multilogin's profile is made here, its proxy replaced after.
	pub async fn set_browser(&self, b: &Browsers, owner: Uuid, id: i64, new: NewBrowser, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		if let Err(r) = b.of(new.provider) {
			return Ok(Err(r));
		}
		match (new.provider, new.device) {
			(Provider::Multilogin, Some(_)) => return Ok(Err(Refused::Invalid("a cloud phone's model is drawn from Multilogin's: send no device".to_owned()))),
			(Provider::Gologin | Provider::MultiloginBrowser, None) => return Ok(Err(Refused::Invalid(format!("a {} browser poses as a device: name one", new.provider)))),
			_ => {}
		}
		let proxy = match ProxyUrl::parse(&new.proxy) {
			Ok(p) => p,
			Err(why) => return Ok(Err(Refused::Invalid(why))),
		};
		let tag = self.key.proxy_tag(&proxy.normalized());
		let (gmail, set) = {
			let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a gmail's browser")?;
			if !is_open(&mut conn).await? {
				return Ok(Err(Refused::Closed));
			}
			let Some(gmail) = gmbs_db::gmails(&mut conn, owner).await?.into_iter().find(|g| g.id == id) else {
				return Ok(Err(Refused::NotFound));
			};
			let set = db::browser(&mut conn, id).await?.map(|(b, _)| b);
			if let Some(set) = &set
				&& (set.provider != new.provider || new.device.is_some_and(|d| set.device != Poses::Device(d)))
			{
				return Ok(Err(Refused::Conflict(format!(
					"this gmail's browser is {} as {}: a fingerprint is set once; remove it to start another",
					set.provider, set.device
				))));
			}
			if db::tag_holder(&mut conn, &tag).await?.is_some_and(|holder| holder != id) {
				return Ok(Err(Refused::Invalid("another gmail leaves through this proxy: one proxy is one gmail's".to_owned())));
			}
			(gmail, set)
		};
		let exit = match b.exits.check(&proxy).await? {
			Ok(e) => e,
			Err(why) => return Ok(Err(Refused::Invalid(why))),
		};
		let place = match Place::try_new(exit.zone.clone(), &exit.country, None) {
			Ok(p) => p,
			Err(why) => return Ok(Err(Refused::Invalid(why))),
		};
		let (device, profile) = match (&set, new.provider, new.device) {
			(Some(set), _, _) => {
				match (set.provider, &set.profile, &set.device) {
					(Provider::Multilogin, Some(phone), _) => said!(b.ml().phone_proxy(phone, &proxy).await?),
					(Provider::MultiloginBrowser, Some(profile), Poses::Device(d)) => said!(b.ml().profile_update(profile, &mimic(&gmail.gmail, *d, &place, &proxy)).await?),
					(Provider::Gologin, ..) => {}
					(p, profile, device) => eyre::bail!("gmail {id}'s {p} browser is stored as {device} with profile {profile:?}"),
				}
				(set.device.clone(), set.profile.clone())
			}
			(None, Provider::Multilogin, None) => {
				let (phone, model) = said!(b.ml().phone(&gmail.gmail, &proxy).await?);
				(Poses::Phone(model), Some(phone))
			}
			(None, Provider::MultiloginBrowser, Some(d)) => {
				let made = said!(b.ml().profile_create(&mimic(&gmail.gmail, d, &place, &proxy)).await?);
				(Poses::Device(d), Some(made))
			}
			(None, Provider::Gologin, Some(d)) => (Poses::Device(d), None),
			_ => unreachable!("device checked against the provider above"),
		};
		let mut tx = self.store.begin_write().await?;
		if let Err(why) = in_countries(&mut tx, id, &exit).await? {
			return Ok(Err(Refused::Invalid(why)));
		}
		if gmbs_db::gmail_owner(&mut tx, id).await? != Some(owner) {
			return Ok(Err(Refused::NotFound));
		}
		// again under the write lock: another gmail may have taken it while the check ran
		if db::tag_holder(&mut tx, &tag).await?.is_some_and(|holder| holder != id) {
			return Ok(Err(Refused::Invalid("another gmail leaves through this proxy: one proxy is one gmail's".to_owned())));
		}
		let stored = Proxy {
			sealed: self.seal(&gmail_browser_aad(id), &new.proxy)?,
			tag,
			endpoint: proxy.endpoint(),
		};
		let f = Fingerprint {
			provider: new.provider,
			device: &device,
			profile: profile.as_deref(),
		};
		db::set(&mut tx, id, f, &stored, &exit, now).await?;
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

	/// The provider's profile (a phone's film with it) and the row: once its runner let it go.
	pub async fn delete_browser(&self, b: &Browsers, owner: Uuid, id: i64) -> eyre::Result<Result<(), Refused>> {
		let browser = match self.browser_of(owner, id).await? {
			Ok(b) => b,
			Err(r) => return Ok(Err(r)),
		};
		if let Err(r) = b.of(browser.provider) {
			return Ok(Err(r));
		}
		match (browser.provider, &browser.profile) {
			(_, None) => {}
			(Provider::Gologin, Some(p)) => said!(b.gl().delete(p).await?),
			(Provider::Multilogin, Some(p)) => {
				if let Some(m) = &browser.film_material {
					said!(b.ml().forget_film(m).await?);
				}
				said!(b.ml().phone_delete(p).await?);
			}
			(Provider::MultiloginBrowser, Some(p)) => said!(b.ml().profile_delete(p).await?),
		}
		let mut tx = self.store.begin_write().await?;
		db::delete(&mut tx, id).await?;
		tx.commit().await.wrap_err("committing a browser's removal")?;
		Ok(Ok(()))
	}

	/// A cloud phone's film, into Multilogin's Library: the operator picks it in its Camera panel.
	/// The one before leaves the Library once this one is in.
	pub async fn set_phone_film(&self, b: &Browsers, owner: Uuid, id: i64, mp4: Vec<u8>) -> eyre::Result<Result<(), Refused>> {
		let browser = match self.phone_of(owner, id).await? {
			Ok(b) => b,
			Err(r) => return Ok(Err(r)),
		};
		if let Err(r) = b.of(Provider::Multilogin) {
			return Ok(Err(r));
		}
		let ml = b.ml();
		let material = said!(ml.upload_film(mp4).await?);
		let mut tx = self.store.begin_write().await?;
		db::set_film(&mut tx, id, Some(&material)).await?;
		tx.commit().await.wrap_err("committing a phone's film")?;
		if let Some(before) = &browser.film_material {
			said!(ml.forget_film(before).await?);
		}
		Ok(Ok(()))
	}

	pub async fn remove_phone_film(&self, b: &Browsers, owner: Uuid, id: i64) -> eyre::Result<Result<(), Refused>> {
		let browser = match self.phone_of(owner, id).await? {
			Ok(b) => b,
			Err(r) => return Ok(Err(r)),
		};
		let Some(material) = &browser.film_material else { return Ok(Ok(())) };
		if let Err(r) = b.of(Provider::Multilogin) {
			return Ok(Err(r));
		}
		said!(b.ml().forget_film(material).await?);
		let mut tx = self.store.begin_write().await?;
		db::set_film(&mut tx, id, None).await?;
		tx.commit().await.wrap_err("committing a phone's film gone")?;
		Ok(Ok(()))
	}

	async fn phone_of(&self, owner: Uuid, id: i64) -> eyre::Result<Result<Browser, Refused>> {
		Ok(self.browser_of(owner, id).await?.and_then(|b| match b.provider {
			Provider::Multilogin => Ok(b),
			p => Err(Refused::Conflict(format!("a {p} browser plays its film from the runner"))),
		}))
	}

	/// What sa_browsers starts the gmail's browser with: at `gmb`'s point, else with the position
	/// blocked; GoLogin's profile made the first time.
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
		if let Err(r) = b.of(browser.provider) {
			return Ok(Err(r));
		}
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
				Place::try_new(zone, &browser.exit.country, Some((point.at.lat, point.at.lng)))
			}
			None => Place::try_new(browser.exit.zone.clone(), &browser.exit.country, None),
		};
		let place = match place {
			Ok(p) => p,
			Err(why) => return Ok(Err(Refused::Conflict(why))),
		};
		let proxy = ProxyUrl::parse(&self.unseal(&gmail_browser_aad(id), &sealed)?).map_err(|why| eyre::eyre!("a stored proxy no longer parses: {why}"))?;
		let run = Run(gmb.as_ref().map(|g| g.id)).to_string();
		tracing::info!(gmail_id = id, gmb_id = ?gmb.as_ref().map(|g| g.id), provider = %browser.provider, zone = place.zone, locale = place.locale, %now, "browser launch prepared");
		let browser_of = match (browser.provider, browser.device, browser.profile) {
			(Provider::Gologin, Poses::Device(device), profile) => {
				let made = match (profile, browser.noise) {
					(Some(p), Some(n)) => (p, n),
					(None, None) => match self.gologin_profile(b, id, &gmail.gmail, device, &place, &proxy).await? {
						Ok(made) => made,
						Err(r) => return Ok(Err(r)),
					},
					half => eyre::bail!("gmail {id}'s GoLogin profile is half there: {half:?}"),
				};
				said!(orbita(b.gl(), &gmail.gmail, device, &place, &proxy, made).await?)
			}
			(Provider::MultiloginBrowser, Poses::Device(device), Some(profile)) => {
				let ml = b.ml();
				said!(ml.profile_update(&profile, &mimic(&gmail.gmail, device, &place, &proxy)).await?);
				sa_browsers::Browser::MultiloginBrowser {
					token: said!(ml.run_token().await?).to_string(),
					folder: said!(ml.folder().await?),
					profile,
				}
			}
			(Provider::Multilogin, Poses::Phone(_), Some(phone)) => sa_browsers::Browser::MultiloginPhone {
				token: said!(b.ml().run_token().await?).to_string(),
				phone,
				geo: place.fix,
				zone: place.zone,
				country: browser.exit.country,
			},
			(p, device, profile) => eyre::bail!("gmail {id}'s {p} browser is stored as {device} with profile {profile:?}"),
		};
		Ok(Ok(Launch { run, browser: browser_of }))
	}

	/// GoLogin's profile, made and its noise read back and kept.
	async fn gologin_profile(&self, b: &Browsers, id: i64, name: &str, device: Device, place: &Place, proxy: &ProxyUrl) -> eyre::Result<Result<(String, Noise), Refused>> {
		let gl = b.gl();
		let made = said!(gl.create(&fingerprint(name, device, place, proxy, None)).await?);
		let noise = said!(gl.noise(&made).await?);
		let mut tx = self.store.begin_write().await?;
		db::set_profile(&mut tx, id, &made, &noise).await?;
		tx.commit().await.wrap_err("committing a browser's provider profile")?;
		tracing::info!(gmail_id = id, profile = made, "GoLogin profile made");
		Ok(Ok((made, noise)))
	}
}

/// Orbita's launch: GoLogin's profile told the fingerprint, its 3-minute token, the preferences.
async fn orbita(gl: &GoLogin, name: &str, device: Device, place: &Place, proxy: &ProxyUrl, (profile, noise): (String, Noise)) -> eyre::Result<Result<sa_browsers::Browser, String>> {
	let hw = device.hardware();
	if let Err(why) = gl.put(&profile, &fingerprint(name, device, place, proxy, Some(&noise))).await? {
		return Ok(Err(why));
	}
	let mut prefs = json!({
		"profile_id": profile,
		"name": name,
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
	let token = match gl.token(&profile).await? {
		Ok(t) => t,
		Err(why) => return Ok(Err(why)),
	};
	prefs["api_domain"] = gl.base.clone().into();
	prefs["profile_token"] = token.as_str().into();
	let languages = place.languages().join(",");
	let Some(app_locale) = sa_browsers::ui_locale(&place.locale) else {
		return Ok(Err(format!("Orbita has no UI locale for {}", place.locale)));
	};
	let config = json!({
		"intl": { "accept_languages": languages, "selected_languages": languages, "app_locale": app_locale, "forced_languages": [place.locale] },
		"gologin": prefs,
	});
	Ok(Ok(sa_browsers::Browser::Orbita {
		preferences,
		config,
		proxy: format!("{}://{}:{}", proxy.scheme, proxy.host, proxy.port),
		zone: place.zone.clone(),
		locale: place.locale.clone(),
		screen: hw.screen,
	}))
}

/// `exit` in the country of every GBP of the gmail that has one.
async fn in_countries(conn: &mut sqlx::SqliteConnection, gmail_id: i64, exit: &Exit) -> eyre::Result<Result<(), String>> {
	let elsewhere: Vec<String> = db::gbp_countries(conn, gmail_id).await?.into_iter().filter(|c| *c != exit.country).collect();
	Ok(match elsewhere.is_empty() {
		true => Ok(()),
		false => Err(format!("the proxy comes out in {}, and the gmail's GBPs are in {}", exit.country, elsewhere.join(", "))),
	})
}

impl std::fmt::Display for Run {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self.0 {
			Some(gmb) => write!(f, "gmb:{gmb}"),
			None => f.write_str("gmail"),
		}
	}
}

impl std::str::FromStr for Run {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, String> {
		match s {
			"gmail" => Ok(Self(None)),
			_ => s
				.strip_prefix("gmb:")
				.and_then(|g| g.parse().ok())
				.map(|g| Self(Some(g)))
				.ok_or_else(|| format!("{s:?} is no run of the conductor's")),
		}
	}
}

/// Where the browser says it is.
struct Place {
	zone: String,
	locale: String,
	/// Latitude, longitude; `None`, the position blocked.
	fix: Option<(f64, f64)>,
}

impl Place {
	fn try_new(zone: String, country: &str, fix: Option<(f64, f64)>) -> Result<Self, String> {
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
	fn try_new(base: &str, token: Zeroizing<String>) -> eyre::Result<Self> {
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

/// Mimic's profile as Multilogin takes it: Mimic draws its navigator (a custom one breaks UA-CH:
/// tmp/ongoing_dev_log/2026-11-10-multilogin.md), the device its GPU and screen, the place its
/// zone, languages and position.
fn mimic(name: &str, device: Device, place: &Place, proxy: &ProxyUrl) -> Value {
	let hw = device.hardware();
	let mut fingerprint = json!({
		"screen": { "width": hw.screen.0, "height": hw.screen.1, "pixel_ratio": hw.dpr },
		"graphic": { "vendor": hw.gpu_vendor, "renderer": hw.gpu_renderer },
		"localization": { "languages": place.locale, "locale": place.locale, "accept_languages": place.accept_language() },
		"timezone": { "zone": place.zone },
	});
	let (geo_masking, geo_popup) = match place.fix {
		Some((latitude, longitude)) => {
			fingerprint["geolocation"] = json!({ "latitude": latitude, "longitude": longitude, "accuracy": GPS_ACCURACY_M, "altitude": 0 });
			("custom", "prompt")
		}
		None => ("mask", "block"),
	};
	json!({
		"name": name,
		"browser_type": "mimic",
		"os_type": "android",
		"parameters": {
			"flags": {
				"audio_masking": "mask", "fonts_masking": "mask", "media_devices_masking": "mask", "navigator_masking": "mask",
				"ports_masking": "mask", "webrtc_masking": "mask", "canvas_noise": "mask", "graphics_noise": "mask",
				"graphics_masking": "custom", "screen_masking": "custom", "localization_masking": "custom", "timezone_masking": "custom",
				"geolocation_masking": geo_masking, "geolocation_popup": geo_popup, "proxy_masking": "custom", "startup_behavior": "recover",
			},
			"fingerprint": fingerprint,
			"storage": { "is_local": true, "save_service_worker": true },
			"proxy": { "type": proxy.scheme, "host": proxy.host, "port": proxy.port, "username": proxy.username, "password": *proxy.password },
		},
	})
}

/// Multilogin's API (MULTILOGIN_TOKEN, an automation token): the phones and Mimic profiles, the
/// Library, and a run's own 3-hour token, which is all the runner gets.
struct Multilogin {
	base: String,
	http: reqwest::Client,
	folder: tokio::sync::OnceCell<String>,
}

impl Multilogin {
	fn try_new(base: &str, token: Zeroizing<String>) -> eyre::Result<Self> {
		let mut auth = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", *token)).wrap_err("MULTILOGIN_TOKEN is not a header value")?;
		auth.set_sensitive(true);
		let headers = reqwest::header::HeaderMap::from_iter([(reqwest::header::AUTHORIZATION, auth)]);
		Ok(Self {
			base: base.to_owned(),
			http: reqwest::Client::builder()
				.default_headers(headers)
				.timeout(Duration::from_secs(60))
				.build()
				.wrap_err("Multilogin's client")?,
			folder: tokio::sync::OnceCell::new(),
		})
	}

	/// `data` of a call that succeeded whole, or what Multilogin said, for the operator; `Err` is
	/// ours. A batch answers 200 with its failures in `fail_amount`.
	async fn call(&self, req: reqwest::RequestBuilder, what: &str) -> eyre::Result<Result<Value, String>> {
		let res = match req.send().await {
			Ok(r) => r,
			Err(e) => return Ok(Err(format!("Multilogin did not answer {what}: {e}"))),
		};
		let status = res.status();
		let text = res.text().await.wrap_err_with(|| format!("reading Multilogin's answer to {what}"))?;
		let refused = || Err(format!("Multilogin refused {what}: {status} {}", text.chars().take(300).collect::<String>()));
		let Ok(v) = serde_json::from_str::<Value>(&text) else { return Ok(refused()) };
		if !status.is_success() || v["status"]["http_code"].as_u64().is_none_or(|c| c >= 400) || v["data"]["fail_amount"].as_u64().is_some_and(|n| n > 0) {
			return Ok(refused());
		}
		Ok(Ok(v["data"].clone()))
	}

	async fn json(&self, method: reqwest::Method, path: &str, body: Value) -> eyre::Result<Result<Value, String>> {
		self.call(self.http.request(method.clone(), format!("{}{path}", self.base)).json(&body), &format!("{method} {path}"))
			.await
	}

	/// Three hours: past a phone's 180 billed minutes, so it can always stop what it started.
	async fn run_token(&self) -> eyre::Result<Result<Zeroizing<String>, String>> {
		let got = self
			.call(self.http.get(format!("{}/workspace/automation_token?expiration_period=3h", self.base)), "a run token")
			.await?;
		Ok(got.and_then(|v| v["token"].as_str().map(|t| Zeroizing::new(t.to_owned())).ok_or_else(|| format!("Multilogin gave no token: {v}"))))
	}

	/// The gmails' folder, made the first time.
	async fn folder(&self) -> eyre::Result<Result<String, String>> {
		if let Some(f) = self.folder.get() {
			return Ok(Ok(f.clone()));
		}
		let listed = match self.call(self.http.get(format!("{}/workspace/folders", self.base)), "the folders").await? {
			Ok(v) => v,
			Err(why) => return Ok(Err(why)),
		};
		let found = listed["folders"]
			.as_array()
			.into_iter()
			.flatten()
			.find(|f| f["name"] == FOLDER)
			.and_then(|f| f["folder_id"].as_str())
			.map(str::to_owned);
		let id = match found {
			Some(id) => id,
			None => match self
				.json(reqwest::Method::POST, "/workspace/folder_create", json!({ "name": FOLDER, "comment": "the conductor's gmails" }))
				.await?
			{
				Ok(v) => match v["id"].as_str() {
					Some(id) => id.to_owned(),
					None => return Ok(Err(format!("Multilogin made a folder and named none: {v}"))),
				},
				Err(why) => return Ok(Err(why)),
			},
		};
		Ok(Ok(self.folder.get_or_init(|| async { id }).await.clone()))
	}

	/// A phone on Android [`ANDROID`] leaving through `proxy`, its model drawn once from the Google
	/// and Samsung ones Multilogin has (a China-only model in a European locale is a tell): its id
	/// and model.
	async fn phone(&self, name: &str, proxy: &ProxyUrl) -> eyre::Result<Result<(String, String), String>> {
		let models = match self
			.json(reqwest::Method::POST, "/mobile_profiles/phone/brand/list", json!({ "android_version": ANDROID }))
			.await?
		{
			Ok(v) => v,
			Err(why) => return Ok(Err(why)),
		};
		let pool: Vec<(&str, &str)> = models
			.as_array()
			.into_iter()
			.flatten()
			.filter_map(|m| Some((m["surface_brand_name"].as_str()?, m["surface_model_name"].as_str()?)))
			.filter(|(brand, _)| ["Google", "Samsung"].contains(brand))
			.collect();
		if pool.is_empty() {
			return Ok(Err(format!("Multilogin has no Google or Samsung model on Android {ANDROID}")));
		}
		let (brand, model) = pool[usize::try_from(getrandom::u32().wrap_err("drawing a phone's model")?).expect("a u32 fits") % pool.len()];
		let made = self
			.json(
				reqwest::Method::POST,
				"/mobile_profiles/phone/create",
				json!({ "mobile_type": format!("Android {ANDROID}"), "data": [{ "profile_name": name, "proxy_information": proxy.normalized(), "surface_brand_name": brand, "surface_model_name": model }] }),
			)
			.await?;
		Ok(made.and_then(|v| {
			let id = v["details"][0]["id"].as_str().ok_or_else(|| format!("Multilogin made a phone and named none: {v}"))?;
			Ok((id.to_owned(), format!("{brand} {model}")))
		}))
	}

	async fn phone_proxy(&self, id: &str, proxy: &ProxyUrl) -> eyre::Result<Result<(), String>> {
		// type_id and protocol: 1 SOCKS5, 2 HTTP
		let kind = if proxy.scheme == "socks5" { 1 } else { 2 };
		let config = json!({ "use_proxy_cfg": false, "type_id": kind, "protocol": kind, "server": proxy.host, "port": proxy.port, "username": proxy.username, "password": *proxy.password });
		Ok(self
			.json(reqwest::Method::PUT, "/mobile_profiles/phone/update", json!({ "id": id, "proxy_config": config }))
			.await?
			.map(drop))
	}

	async fn phone_delete(&self, id: &str) -> eyre::Result<Result<(), String>> {
		Ok(self.json(reqwest::Method::DELETE, "/mobile_profiles/phone/delete", json!({ "ids": [id] })).await?.map(drop))
	}

	async fn profile_create(&self, profile: &Value) -> eyre::Result<Result<String, String>> {
		let folder = match self.folder().await? {
			Ok(f) => f,
			Err(why) => return Ok(Err(why)),
		};
		let mut body = profile.clone();
		body["folder_id"] = folder.into();
		Ok(self
			.json(reqwest::Method::POST, "/profile/create", body)
			.await?
			.and_then(|v| v["ids"][0].as_str().map(str::to_owned).ok_or_else(|| format!("Multilogin made a profile and named none: {v}"))))
	}

	async fn profile_update(&self, id: &str, profile: &Value) -> eyre::Result<Result<(), String>> {
		let mut body = profile.clone();
		body["profile_id"] = id.into();
		Ok(self.json(reqwest::Method::POST, "/profile/update", body).await?.map(drop))
	}

	async fn profile_delete(&self, id: &str) -> eyre::Result<Result<(), String>> {
		Ok(self.json(reqwest::Method::POST, "/profile/remove", json!({ "ids": [id], "permanently": true })).await?.map(drop))
	}

	async fn upload_film(&self, mp4: Vec<u8>) -> eyre::Result<Result<String, String>> {
		let part = reqwest::multipart::Part::bytes(mp4).file_name("film.mp4").mime_str("video/mp4").expect("a valid mime");
		let req = self
			.http
			.post(format!("{}/mobile_profiles/files/upload", self.base))
			.timeout(Duration::from_secs(600))
			.multipart(reqwest::multipart::Form::new().part("file", part));
		Ok(self
			.call(req, "a film's upload")
			.await?
			.and_then(|v| v["material_id"].as_str().map(str::to_owned).ok_or_else(|| format!("Multilogin kept a film and named none: {v}"))))
	}

	async fn forget_film(&self, material: &str) -> eyre::Result<Result<(), String>> {
		Ok(self.json(reqwest::Method::POST, "/mobile_profiles/files/delete", json!({ "ids": [material] })).await?.map(drop))
	}
}
