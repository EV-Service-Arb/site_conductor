//! The change feed's two ends agree: what `feeds::changes` streams parses as `feeds::Line`.

use http_body_util::BodyExt;
use sa_core::feeds::{Line, changes};
use tokio::sync::broadcast;

#[tokio::test]
async fn the_feed_opens_with_resync_streams_topics_and_ends_with_its_stop() {
	let (tx, rx) = broadcast::channel(4);
	let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
	let res = changes(rx, async move { stopped.await.expect("sent before the test ends") });
	assert_eq!(res.headers()["content-type"], "application/x-ndjson");
	tx.send("sites.leads".to_owned()).unwrap();
	tx.send("sites.places".to_owned()).unwrap();
	let mut body = res.into_body();
	let mut lines = Vec::new();
	let mut pending = Vec::new();
	while lines.len() < 3 {
		let frame = body.frame().await.expect("open").unwrap().into_data().unwrap();
		pending.extend_from_slice(&frame);
		while let Some(end) = pending.iter().position(|b| *b == b'\n') {
			let line: Vec<u8> = pending.drain(..=end).collect();
			lines.push(serde_json::from_slice::<Line>(&line).unwrap());
		}
	}
	assert_eq!(lines, [Line::Resync, Line::Changed { topic: "sites.leads".into() }, Line::Changed { topic: "sites.places".into() }]);
	stop.send(()).unwrap();
	assert!(body.frame().await.is_none(), "the stop ends the stream");
}

#[tokio::test]
async fn a_reader_behind_the_channel_is_told_to_resync() {
	let (tx, rx) = broadcast::channel(1);
	tx.send("a.x".to_owned()).unwrap();
	tx.send("a.y".to_owned()).unwrap();
	let mut body = changes(rx, std::future::pending()).into_body();
	let mut text = String::new();
	while text.lines().count() < 3 {
		text.push_str(std::str::from_utf8(&body.frame().await.unwrap().unwrap().into_data().unwrap()).unwrap());
	}
	let lines: Vec<Line> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
	assert_eq!(lines, [Line::Resync, Line::Resync, Line::Changed { topic: "a.y".into() }], "a.x was missed: the reader re-reads all");
	drop(tx);
	assert!(body.frame().await.is_none(), "a closed channel ends the stream");
}
