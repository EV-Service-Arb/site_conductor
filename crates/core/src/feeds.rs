//! What the conductor's hubs read of a service and push to it: `/api/conductor/*`, each under a
//! Conductor assertion. A path and the page it answers are named once, here, for both ends.
//!
//! ```text
//! GET  CHANGES                  NDJSON of Line, open until the service stops     sites, review_archive
//! GET  NOTIFICATIONS?after&limit  sa_alerts::NotePage                            sites, review_archive
//! GET  CHARGES?after&limit      tokens::ChargePage                               review_archive
//! GET  USAGE?after&limit        usage::UsagePage                                 review_archive
//! GET  REVIEWS?after&limit      ReviewPage                                       review_archive
//! PUT  GMBS                     gbp::Snapshot                                    review_archive
//! GET  GONE                     Vec<GoneReview>, 404: it tracks no such GBP      review_archive
//! GET  CAPTURES/<sha>.png       a note's photo                                   review_archive
//! GET  PLACES                   Places                                           sites
//! GET  SOURCES_RECEIVED         Received                                         sites
//! GET  INTERNAL_READS           InternalReads                                    sites
//! ```

use std::collections::BTreeMap;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

pub const CHANGES: &str = "/api/conductor/changes";
pub const NOTIFICATIONS: &str = "/api/conductor/notifications";
pub const CHARGES: &str = "/api/conductor/charges";
pub const USAGE: &str = "/api/conductor/usage";
pub const REVIEWS: &str = "/api/conductor/reviews";
pub const GMBS: &str = "/api/conductor/gmbs";
pub const GONE: &str = "/api/conductor/gmbs/{gmb}/gone";
pub const CAPTURES: &str = "/api/conductor/captures";
pub const PLACES: &str = "/api/conductor/places";
pub const SOURCES_RECEIVED: &str = "/api/conductor/sources/received";
pub const INTERNAL_READS: &str = "/api/conductor/internal-reads";

/// The most [`GONE`] answers.
pub const GONE_MAX: u32 = 100;

/// Each source key's last event's arrival.
pub type Received = BTreeMap<String, Timestamp>;

#[cfg(feature = "api")]
pub use stream::changes;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrandReads {
	pub locations: Option<Timestamp>,
	pub pricing: Option<Timestamp>,
	pub experiments: Option<Timestamp>,
}

/// The last `/api/internal` read per brand since `since` (the service's boot); an endpoint is absent
/// when not read since.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InternalReads {
	pub since: Timestamp,
	pub reads: BTreeMap<String, BrandReads>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
	pub gmb_id: i64,
	pub brand: String,
	pub slug: String,
}

/// Every live place with a GBP: which landing page is which GBP.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Places {
	pub places: Vec<Place>,
}

/// A review of a GBP the archive saw appear, go or come back, in the order it saw it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
	pub seq: i64,
	/// Google's id of the review.
	pub review_id: String,
	pub gmb_id: i64,
	/// 1 to 5.
	pub rating: u8,
	pub text: String,
	pub first_seen: Timestamp,
	/// Not seen on its last walk; `None` again when it came back.
	pub gone_at: Option<Timestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewPage {
	/// Ascending by `seq`; a review again each time it appears, goes or comes back.
	pub reviews: Vec<Review>,
	/// The last `seq` of the page; `after` for an empty one.
	pub next: i64,
}

/// A review of a GBP its last covering walk did not list, newest gone first.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoneReview {
	/// Google's id of the review.
	pub review_id: String,
	pub author: String,
	/// The author's Google profile.
	pub author_url: Option<String>,
	/// 1 to 5.
	pub rating: Option<u8>,
	pub text: Option<String>,
	/// Google's date as it printed it, relative to when it was read.
	pub published_raw: Option<String>,
	/// What `published_raw` was estimated to be.
	pub published_est: Option<Timestamp>,
	pub first_seen: Timestamp,
	pub gone_at: Timestamp,
	pub captured: bool,
	/// By the owner.
	pub replied: bool,
	/// The open reinstatement request's.
	pub reinstatement_requested_at: Option<Timestamp>,
}

/// A line of a service's change feed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Line {
	/// A table the service committed a write to: `<service>.<table>`.
	Changed { topic: String },
	/// Whatever was read may have changed: read it all again.
	Resync,
	/// Nothing happened; the feed is alive.
	Ping,
}

#[cfg(feature = "api")]
mod stream {
	use std::time::Duration;

	use axum::{
		body::{Body, Bytes},
		http::{HeaderValue, header},
		response::{IntoResponse, Response},
	};
	use tokio::sync::broadcast::{Receiver, error::RecvError};

	use super::Line;

	/// A line at least this often, so the conductor tells a quiet feed from a dead one.
	const PING_EVERY: Duration = Duration::from_secs(25);

	/// [`super::CHANGES`]: `resync` first, then each topic `topics` carries as it comes, `resync` when
	/// the reader fell behind the channel, `ping` every 25 s; ends when `stop` does or the channel
	/// closes. Subscribe before calling: whatever commits after the first line is in the stream.
	pub fn changes(topics: Receiver<String>, stop: impl Future<Output = ()> + Send + 'static) -> Response {
		let mut ping = tokio::time::interval_at(tokio::time::Instant::now() + PING_EVERY, PING_EVERY);
		ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
		let first = futures::stream::once(async { Ok::<_, std::convert::Infallible>(bytes(&Line::Resync)) });
		let rest = futures::stream::unfold((topics, ping, Box::pin(stop)), |(mut topics, mut ping, mut stop)| async move {
			let line = tokio::select! {
				topic = topics.recv() => match topic {
					Ok(topic) => Line::Changed { topic },
					Err(RecvError::Lagged(_)) => Line::Resync,
					Err(RecvError::Closed) => return None,
				},
				_ = ping.tick() => Line::Ping,
				() = &mut stop => return None,
			};
			Some((Ok(bytes(&line)), (topics, ping, stop)))
		});
		let mut res = Body::from_stream(futures::StreamExt::chain(first, rest)).into_response();
		res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/x-ndjson"));
		res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
		res
	}

	fn bytes(line: &Line) -> Bytes {
		let mut out = serde_json::to_vec(line).expect("a line serializes");
		out.push(b'\n');
		out.into()
	}
}
