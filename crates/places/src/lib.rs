//! A Google place from what a person pastes. The id, or a Maps URL carrying it, needs nothing;
//! a URL with only a name in it is looked up with the Places API (New). Every Places request
//! goes through [`Google`]: the queries cache answers what was paid for once; else the [`Payer`]'s
//! own keys are asked first, then the [`Keys`] rotation, which its [`Purse`] pays for.

mod keys;
pub mod parse;
mod queries_cache;

use std::{sync::Arc, time::Duration};

use keys::Tier;
pub use keys::{Keys, Spent};
use parse::Parsed;
use queries_cache::Request;
pub use queries_cache::{Pending, QueriesCache, Remote};
use sa_core::queries_cache::{Claim, Put};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const SEARCH_MASK: &str = "places.id,places.displayName,places.formattedAddress,places.location,places.addressComponents";

const DETAILS_MASK: &str = "id,displayName,formattedAddress,location,addressComponents";

const AUTOCOMPLETE_MASK: &str = "suggestions.placePrediction.placeId,suggestions.placePrediction.structuredFormat";
/// A GMB created on Google today is findable by name tomorrow.
const SEARCH_AGE: Duration = Duration::from_secs(86_400);
/// The place a key is tried on: Google's own office, the one its docs ask about.
const PROBE_PLACE: &str = "ChIJj61dQgK6j4AR4GeTYWZsKWw";

/// Pays for buys on the rotation.
pub trait Purse: Send + Sync {
	/// Room for one buy of `sku`, kept until paid or dropped; else [`Error::Unpaid`].
	fn hold(&self, sku: Sku) -> Pending<'_, Box<dyn Hold>>;
}

/// A purse's room for one buy; dropped unpaid, it is given back.
pub trait Hold: Send {
	/// The buy went through.
	fn paid(self: Box<Self>, bought: Bought) -> Pending<'static, ()>;
}

/// How Google bills a request, by what its field mask asks: both masks ask a display name, which
/// makes them the Pro SKUs.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::Display, strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(serialize_all = "snake_case")]
pub enum Sku {
	/// Place Details Pro.
	Details,
	/// Text Search Pro.
	Search,
	/// Autocomplete Requests: per request, no session token.
	Autocomplete,
}

/// Who a request is made for.
pub struct Payer<'a> {
	/// Keys of their own, in the order tried, before any of the rotation: free.
	pub own: Vec<Zeroizing<String>>,
	/// Pays for a buy on the rotation; `None`, the house does.
	pub purse: Option<&'a dyn Purse>,
}
impl Payer<'_> {
	/// No keys of anyone's: the rotation, the house paying.
	pub fn house() -> Self {
		Self { own: Vec::new(), purse: None }
	}
}

/// A buy on the rotation, as its payer is charged for it.
#[derive(Clone, Debug)]
pub struct Bought {
	/// The query's, as the queries cache keys it.
	pub key: sa_core::queries_cache::Key,
	pub sku: Sku,
	/// What was asked, as a person reads it.
	pub label: String,
}
/// A Google place, as Places answers for it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Place {
	pub place_id: String,
	/// Its display name.
	pub name: Option<String>,
	pub address: Option<String>,
	/// None for a service-area business: Google shows it no point.
	pub location: Option<LatLng>,
	/// ISO 3166-1 alpha-2; None for a service-area business, as its point.
	pub country: Option<String>,
}

/// A place an input being typed may be heading for, as Google's own search box offers it.
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
	pub place_id: String,
	/// Its name, or the address's first line.
	pub main: String,
	/// The rest of its address; Google gives some none.
	pub secondary: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct LatLng {
	pub lat: f64,
	pub lng: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// What the person pasted names no one place: theirs to fix.
	#[error("{0}")]
	Invalid(String),
	/// The Places API refused a request.
	#[error("Places API, {asked}: {status}: {body}")]
	Api { asked: String, status: u16, body: String },
	/// The Places API refused a key of the payer's own, for something else than a spent window.
	#[error("Google refuses the Maps key …{tail}: {said}")]
	OwnKey { tail: String, said: String },
	/// The purse refused a buy on the rotation: why, as the payer reads it.
	#[error("{0}")]
	Unpaid(String),
	/// The purse failed to answer.
	#[error("the purse: {0}")]
	Purse(String),
	/// Every key is sitting out.
	#[error("every Places key is spent: {}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
	Exhausted(Vec<Spent>),
	/// The request itself failed.
	#[error(transparent)]
	Http(#[from] reqwest::Error),
	/// An answer, paid or cached, is not the shape asked for.
	#[error("decoding a Places answer")]
	Decode(#[from] serde_json::Error),
	/// The queries cache failed, or did not answer.
	#[error("the queries cache: {0}")]
	QueriesCache(String),
}

/// Whether a kept answer will do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ask {
	Cached,
	/// Asked again, and the kept answer replaced.
	Afresh,
}

/// The Places API (New), through the queries cache and the keys.
#[derive(Debug)]
pub struct Google {
	http: reqwest::Client,
	keys: Keys,
	cache: Arc<dyn QueriesCache>,
	/// `…/v1/places`.
	base: String,
}

impl Google {
	/// With no keys, a place id still resolves, unnamed and unlocated.
	pub fn try_new(keys: Keys, cache: Arc<dyn QueriesCache>) -> Result<Self, Error> {
		let http = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(10)).build()?;
		Ok(Self {
			http,
			keys,
			cache,
			base: "https://places.googleapis.com/v1/places".to_owned(),
		})
	}

	/// Places at `base` (`…/v1/places`) in place of Google's own: a stub's.
	pub fn at(self, base: impl Into<String>) -> Self {
		Self { base: base.into(), ..self }
	}

	/// The place id in `input`, and the place unless there is no key: one place only, an
	/// ambiguous name is refused with what it may mean.
	pub async fn resolve(&self, input: &str, payer: &Payer<'_>) -> Result<(String, Option<Place>), Error> {
		match parsed(input).await? {
			Parsed::PlaceId(id) if self.keyless(payer) => Ok((id, None)),
			Parsed::PlaceId(id) | Parsed::Alias(id) => {
				let place = self.details(&id, Ask::Cached, payer).await?;
				Ok((place.place_id.clone(), Some(place)))
			}
			Parsed::Search { query, near } => match <[Place; 1]>::try_from(self.search(&query, near, payer).await?) {
				Ok([one]) => Ok((one.place_id.clone(), Some(one))),
				Err(many) => {
					let each = many
						.iter()
						.map(|p| [Some(p.place_id.as_str()), p.address.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(", "));
					Err(Error::Invalid(format!(
						"{} places are named {query:?}; add one by its id: {}",
						many.len(),
						each.collect::<Vec<_>>().join("; ")
					)))
				}
			},
		}
	}

	/// Every place `input` may mean, each as [`Self::resolve`] gives one: the one its id names,
	/// else each Places hit bearing the name in the URL.
	pub async fn candidates(&self, input: &str, payer: &Payer<'_>) -> Result<Vec<(String, Option<Place>)>, Error> {
		let places = match parsed(input).await? {
			Parsed::PlaceId(id) if self.keyless(payer) => return Ok(vec![(id, None)]),
			Parsed::PlaceId(id) | Parsed::Alias(id) => vec![self.details(&id, Ask::Cached, payer).await?],
			Parsed::Search { query, near } => self.search(&query, near, payer).await?,
		};
		Ok(places.into_iter().map(|p| (p.place_id.clone(), Some(p))).collect())
	}

	/// The place with this id, under the id Google now gives it: a moved or merged place answers
	/// with its newer one.
	pub async fn details(&self, place_id: &str, ask: Ask, payer: &Payer<'_>) -> Result<Place, Error> {
		let url = format!("{}/{place_id}", self.base);
		let (json, _) = match self.ask(&Request::details(&url, place_id), None, ask, payer).await {
			Err(Error::Api { status: 400 | 404, .. }) => return Err(Error::Invalid(format!("Places knows no place {place_id:?}"))),
			r => r?,
		};
		Ok(serde_json::from_value::<Hit>(json)?.into())
	}

	/// The places `input` may be heading for, within `regions` (ISO 3166-1 alpha-2, at most 15).
	/// No session token: each answer is its request's, so the queries cache serves it again.
	pub async fn suggest(&self, input: &str, regions: &[&str], payer: &Payer<'_>) -> Result<Vec<Suggestion>, Error> {
		#[derive(Deserialize)]
		struct Resp {
			#[serde(default)]
			suggestions: Vec<Suggested>,
		}
		#[derive(Deserialize)]
		#[serde(rename_all = "camelCase")]
		struct Suggested {
			place_prediction: Option<Prediction>,
		}
		#[derive(Deserialize)]
		#[serde(rename_all = "camelCase")]
		struct Prediction {
			place_id: String,
			structured_format: Structured,
		}
		#[derive(Deserialize)]
		#[serde(rename_all = "camelCase")]
		struct Structured {
			main_text: Text,
			secondary_text: Option<Text>,
		}

		let input = input.split_whitespace().collect::<Vec<_>>().join(" ");
		if input.is_empty() {
			return Err(Error::Invalid("nothing typed to suggest places for".to_owned()));
		}
		let (url, body) = (
			format!("{}:autocomplete", self.base),
			serde_json::json!({ "input": input, "includedRegionCodes": regions }).to_string(),
		);
		let req = Request {
			method: "POST",
			url: &url,
			mask: AUTOCOMPLETE_MASK,
			body: &body,
			sku: Sku::Autocomplete,
			subject: &input,
		};
		let (json, _) = self.ask(&req, Some(SEARCH_AGE), Ask::Cached, payer).await?;
		Ok(serde_json::from_value::<Resp>(json)?
			.suggestions
			.into_iter()
			.filter_map(|s| s.place_prediction) // a query prediction names no place; none asked for, Google may still send one
			.map(|p| Suggestion {
				place_id: p.place_id,
				main: p.structured_format.main_text.text,
				secondary: p.structured_format.secondary_text.map(|t| t.text),
			})
			.collect())
	}

	/// Text search: every hit bearing the name searched for, best first. Each place it pays for is
	/// cached as that place's details.
	async fn search(&self, query: &str, near: Option<(f64, f64)>, payer: &Payer<'_>) -> Result<Vec<Place>, Error> {
		#[derive(Deserialize)]
		struct Resp {
			#[serde(default)]
			places: Vec<serde_json::Value>,
		}

		let mut body = serde_json::json!({ "textQuery": query, "pageSize": 20 });
		if let Some((lat, lng)) = near {
			body["locationBias"] = serde_json::json!({ "circle": { "center": { "latitude": lat, "longitude": lng }, "radius": 500.0 } });
		}
		let (url, body) = (format!("{}:searchText", self.base), body.to_string());
		let req = Request {
			method: "POST",
			url: &url,
			mask: SEARCH_MASK,
			body: &body,
			sku: Sku::Search,
			subject: query,
		};
		let (json, paid) = self.ask(&req, Some(SEARCH_AGE), Ask::Cached, payer).await?;
		let mut named = Vec::new();
		for raw in serde_json::from_value::<Resp>(json)?.places {
			let place = Place::from(serde_json::from_value::<Hit>(raw.clone())?);
			if paid {
				let details = format!("{}/{}", self.base, place.place_id);
				let req = Request::details(&details, &place.place_id);
				self.cache
					.put(
						req.key(),
						Put {
							answer: raw.clone(),
							request: req.preimage(),
						},
					)
					.await?;
			}
			if place.name.as_ref().is_some_and(|n| n.eq_ignore_ascii_case(query)) {
				named.push(place);
			}
		}
		if named.is_empty() {
			return Err(Error::Invalid(format!("Places found no place named {query:?}; paste a URL with its id")));
		}
		Ok(named)
	}

	/// The answer to `req`, and whether it was paid for now: from the cache unless older than
	/// `max_age`, else claimed there and bought with the first key not sitting out.
	async fn ask(&self, req: &Request<'_>, max_age: Option<Duration>, ask: Ask, payer: &Payer<'_>) -> Result<(serde_json::Value, bool), Error> {
		let key = req.key();
		let now = jiff::Timestamp::now();
		let fresh_since = match ask {
			Ask::Cached => max_age.map(|age| now - jiff::SignedDuration::try_from(age).expect("an age of seconds")),
			Ask::Afresh => Some(now),
		};
		if ask == Ask::Cached
			&& let Some(kept) = self.cache.get(key).await?
			&& fresh_since.is_none_or(|since| kept.fetched_at >= since)
		{
			return Ok((kept.answer, false));
		}
		if self.keyless(payer) {
			return Err(Error::Invalid("asking Google Places needs an API key, and none is set".into()));
		}
		loop {
			match self.cache.claim(key, fresh_since).await? {
				Claim::Stored(kept) => return Ok((kept.answer, false)),
				Claim::Held { .. } => tokio::time::sleep(Duration::from_millis(500)).await,
				Claim::Granted { .. } => break,
			}
		}
		let bought = self.buy(req, payer).await;
		if bought.is_err()
			&& let Err(e) = self.cache.release(key).await
		{
			// the buy's error is the one to tell; the claim lapses with its lease anyway
			tracing::warn!(error = %e, "releasing a claim after a failed buy");
		}
		let (answer, hold) = bought?;
		self.cache
			.put(
				key,
				Put {
					answer: answer.clone(),
					request: req.preimage(),
				},
			)
			.await?;
		if let Some(hold) = hold {
			let label = match req.sku {
				Sku::Details => format!("Places details · {}", req.subject),
				Sku::Search => format!("Places search · {:?}", req.subject),
				Sku::Autocomplete => format!("Places autocomplete · {:?}", req.subject),
			};
			hold.paid(Bought { key, sku: req.sku, label }).await?;
		}
		Ok((answer, true))
	}

	/// `req` asked of Google with the first key not sitting out: the payer's own, then the
	/// rotation's, held in their purse before the first of those is asked; the hold, to be paid
	/// once the answer is kept.
	async fn buy(&self, req: &Request<'_>, payer: &Payer<'_>) -> Result<(serde_json::Value, Option<Box<dyn Hold>>), Error> {
		let mut tried = Vec::new();
		let mut hold: Option<Box<dyn Hold>> = None;
		loop {
			let (api_key, tier) = self.keys.next(&payer.own, &tried).map_err(Error::Exhausted)?;
			if tier == Tier::Rotation
				&& hold.is_none()
				&& let Some(purse) = payer.purse
			{
				hold = Some(purse.hold(req.sku).await?);
			}
			let r = match req.method {
				"POST" => self.http.post(req.url).header(reqwest::header::CONTENT_TYPE, "application/json").body(req.body.to_owned()),
				"GET" => self.http.get(req.url),
				other => unreachable!("Places is asked by GET and POST only, not {other}"),
			};
			let resp = r.header("X-Goog-Api-Key", api_key.as_str()).header("X-Goog-FieldMask", req.mask).send().await?;
			let status = resp.status();
			tracing::info!(method = req.method, url = req.url, %status, ?tier, "Places asked");
			let text = resp.text().await?;
			if status.is_success() {
				let answer = serde_json::from_str(&text).map_err(|e| Error::Api {
					asked: format!("{} {}", req.method, req.url),
					status: 200,
					body: format!("not JSON ({e}), paid for and not kept: {text}"),
				})?;
				return Ok((answer, hold));
			}
			match (Spent::of(&text), tier) {
				(Some(why), _) => {
					self.keys.sit_out(&api_key, why);
					tried.push(api_key);
				}
				(None, Tier::Own) => {
					return Err(Error::OwnKey {
						tail: keys::tail(&api_key).to_owned(),
						said: said(&text),
					});
				}
				(None, Tier::Rotation) => {
					return Err(Error::Api {
						asked: format!("{} {}", req.method, req.url),
						status: status.as_u16(),
						body: text,
					});
				}
			}
		}
	}

	/// Whether `key` answers for Places: a details request asking only the id, which Google does
	/// not bill, so it is neither cached nor paid for. Answering, it is back in at once.
	pub async fn probe(&self, key: &str) -> Result<(), Error> {
		let resp = self
			.http
			.get(format!("{}/{PROBE_PLACE}", self.base))
			.header("X-Goog-Api-Key", key)
			.header("X-Goog-FieldMask", "id")
			.send()
			.await?;
		let status = resp.status();
		let text = resp.text().await?;
		tracing::info!(key = %format!("…{}", keys::tail(key)), %status, "Places key probed");
		if status.is_success() {
			self.keys.back(key);
			return Ok(());
		}
		Err(Error::OwnKey {
			tail: keys::tail(key).to_owned(),
			said: said(&text),
		})
	}

	/// Why an own key sits out, if it does.
	pub fn sat_out(&self, key: &str) -> Option<Spent> {
		self.keys.out(key)
	}

	fn keyless(&self, payer: &Payer<'_>) -> bool {
		payer.own.is_empty() && self.keys.is_empty()
	}
}

/// Google's message in a refusal, else the refusal as it came.
fn said(text: &str) -> String {
	// not JSON, or no message in it: the body is all there is to tell
	serde_json::from_str::<serde_json::Value>(text)
		.ok()
		.and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
		.unwrap_or_else(|| text.to_owned())
}

impl<'a> Request<'a> {
	fn details(url: &'a str, place_id: &'a str) -> Self {
		Self {
			method: "GET",
			url,
			mask: DETAILS_MASK,
			body: "",
			sku: Sku::Details,
			subject: place_id,
		}
	}
}

/// A place as Places spells it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hit {
	id: String,
	display_name: Option<Text>,
	formatted_address: Option<String>,
	location: Option<Location>,
	#[serde(default)]
	address_components: Vec<Component>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Component {
	short_text: String,
	types: Vec<String>,
}

#[derive(Deserialize)]
struct Text {
	text: String,
}

#[derive(Deserialize)]
struct Location {
	latitude: f64,
	longitude: f64,
}

impl From<Hit> for Place {
	fn from(h: Hit) -> Self {
		Self {
			place_id: h.id,
			name: h.display_name.map(|t| t.text),
			address: h.formatted_address,
			location: h.location.map(|l| LatLng { lat: l.latitude, lng: l.longitude }),
			country: h.address_components.into_iter().find(|c| c.types.iter().any(|t| t == "country")).map(|c| c.short_text),
		}
	}
}

async fn parsed(input: &str) -> Result<Parsed, Error> {
	let mut expanded = input.trim().to_owned();
	for _ in 0..3 {
		if !["https://maps.app.goo.gl/", "https://share.google/", "https://www.google.com/share.google?"]
			.iter()
			.any(|p| expanded.starts_with(p))
		{
			break;
		}
		expanded = expand_short_link(&expanded).await?;
	}
	parse::parse(&expanded).map_err(|e| Error::Invalid(format!("{e:#}")))
}

/// One hop: following further can land on consent.google.com instead of the place.
async fn expand_short_link(url: &str) -> Result<String, Error> {
	let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build()?;
	let resp = client.get(url).send().await?;
	let location = resp
		.headers()
		.get(reqwest::header::LOCATION)
		.ok_or_else(|| Error::Invalid(format!("{url:?} answered {} without a redirect", resp.status())))?;
	Ok(location
		.to_str()
		.map_err(|e| Error::Invalid(format!("{url:?} redirected to a non-UTF-8 location: {e}")))?
		.to_owned())
}
