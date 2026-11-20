//! `SA_TRACE=<file>`: each closed span appended to it as a Chrome trace event, one a line, named with the
//! nextest test that ran it; `test-timeline` (scripts/test.sh) nests them under that test.

use std::{
	fs::File,
	io::Write,
	path::Path,
	sync::Mutex,
	time::{Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{Map, Value, json};
use tracing::{
	Subscriber,
	field::{Field, Visit},
	span,
};
use tracing_subscriber::{Layer, filter::LevelFilter, layer::Context, registry::LookupSpan};

/// None without `SA_TRACE`.
pub fn layer<S: Subscriber + for<'a> LookupSpan<'a>>() -> Option<impl Layer<S>> {
	let path = std::env::var_os("SA_TRACE")?;
	let file = File::options()
		.create(true)
		.append(true)
		.open(&path)
		.unwrap_or_else(|e| panic!("SA_TRACE={}: {e}", Path::new(&path).display()));
	let nextest = |k| std::env::var(k).map_or(Value::Null, Value::String); // outside nextest: no test to nest under
	Some(
		Spans {
			file: Mutex::new(file),
			test: nextest("NEXTEST_TEST_NAME"),
			binary: nextest("NEXTEST_BINARY_ID"),
		}
		.with_filter(LevelFilter::INFO),
	)
}

struct Spans {
	file: Mutex<File>,
	test: Value,
	binary: Value,
}

struct Opened {
	at: Instant,
	ts: u128,
	args: Map<String, Value>,
}

struct Fields<'a>(&'a mut Map<String, Value>);
impl Visit for Fields<'_> {
	fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
		self.0.insert(field.name().into(), format!("{value:?}").into());
	}

	fn record_str(&mut self, field: &Field, value: &str) {
		self.0.insert(field.name().into(), value.into());
	}

	fn record_u64(&mut self, field: &Field, value: u64) {
		self.0.insert(field.name().into(), value.into());
	}

	fn record_i64(&mut self, field: &Field, value: i64) {
		self.0.insert(field.name().into(), value.into());
	}
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Spans {
	fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
		let mut args = Map::new();
		attrs.record(&mut Fields(&mut args));
		let ts = SystemTime::now().duration_since(UNIX_EPOCH).expect("the clock is past 1970").as_micros();
		ctx.span(id).expect("a new span is in the registry").extensions_mut().insert(Opened { at: Instant::now(), ts, args });
	}

	fn on_record(&self, id: &span::Id, values: &span::Record<'_>, ctx: Context<'_, S>) {
		let span = ctx.span(id).expect("a recorded span is in the registry");
		let mut ext = span.extensions_mut();
		values.record(&mut Fields(&mut ext.get_mut::<Opened>().expect("on_new_span put it").args));
	}

	fn on_close(&self, id: span::Id, ctx: Context<'_, S>) {
		let span = ctx.span(&id).expect("a closing span is in the registry");
		let ext = span.extensions();
		let o = ext.get::<Opened>().expect("on_new_span put it");
		let mut args = o.args.clone();
		args.insert("test".into(), self.test.clone());
		args.insert("binary".into(), self.binary.clone());
		let pid = std::process::id();
		let event = json!({"name": span.name(), "cat": span.metadata().target(), "ph": "X", "ts": o.ts, "dur": o.at.elapsed().as_micros(), "pid": pid, "tid": pid, "args": args});
		let mut line = event.to_string();
		line.push('\n');
		self.file
			.lock()
			.expect("never held across a panic")
			.write_all(line.as_bytes()) // one write of an O_APPEND file: the processes' lines don't interleave
			.expect("appending to SA_TRACE");
	}
}
