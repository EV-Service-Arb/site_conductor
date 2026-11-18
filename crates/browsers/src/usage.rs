//! What the runner spent on vendors (`GET /usage`, `sa_core::usage`): `usage.jsonl` in
//! `BROWSERS_DIR`, append-only, one [`Used`] a line, line n's id n. Held in memory whole: a page
//! is a slice by id.

use std::{
	fs::File,
	io::{Seek, SeekFrom, Write},
	path::{Path, PathBuf},
	sync::Mutex,
};

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::{prices::Item, usage::Used};
use uuid::Uuid;

const FILE: &str = "usage.jsonl";

pub(crate) struct Usage {
	path: PathBuf,
	/// Held across an append, so ids follow the file's lines.
	inner: Mutex<Inner>,
}
impl Usage {
	pub(crate) fn open(dir: &Path) -> eyre::Result<Self> {
		let path = dir.join(FILE);
		let text = match std::fs::read_to_string(&path) {
			Ok(t) => t,
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
			Err(e) => return Err(eyre::Report::new(e).wrap_err(format!("reading {}", path.display()))),
		};
		// a line without its newline is an append the runner died in: never served, so dropped
		let whole = text.rfind('\n').map_or(0, |i| i + 1);
		if whole < text.len() {
			tracing::warn!(path = %path.display(), torn = &text[whole..], "a torn last line dropped");
		}
		let mut used = Vec::new();
		for (i, line) in text[..whole].lines().enumerate() {
			let u: Used = serde_json::from_str(line).wrap_err_with(|| format!("{} line {} is not the runner's", path.display(), i + 1))?;
			eyre::ensure!(u.id == i as i64 + 1, "{} line {} has id {}", path.display(), i + 1, u.id);
			used.push(u);
		}
		let mut file = std::fs::OpenOptions::new()
			.create(true)
			.write(true)
			.truncate(false)
			.open(&path)
			.wrap_err_with(|| format!("opening {}", path.display()))?;
		let len = whole as u64;
		file.set_len(len).wrap_err_with(|| format!("dropping {}'s torn line", path.display()))?;
		file.seek(SeekFrom::End(0))?;
		Ok(Self {
			path,
			inner: Mutex::new(Inner { file, len, used }),
		})
	}

	/// On disk before it returns.
	pub(crate) fn record(&self, item: Item, units: i64, at: Timestamp, user: Uuid) -> eyre::Result<()> {
		assert!(units > 0, "{item}: {units} units");
		let mut inner = self.inner.lock().expect("never held across a panic");
		let Inner { file, len, used } = &mut *inner;
		let u = Used {
			id: used.len() as i64 + 1,
			item,
			units,
			at,
			user: Some(user),
		};
		let mut line = serde_json::to_vec(&u)?;
		line.push(b'\n');
		if let Err(e) = file.write_all(&line).and_then(|()| file.sync_data()) {
			if let Err(cut) = file.set_len(*len).and_then(|()| file.seek(SeekFrom::End(0))) {
				panic!("cutting {} back to its whole lines: {cut}", self.path.display());
			}
			return Err(eyre::Report::new(e).wrap_err(format!("appending to {}", self.path.display())));
		}
		*len += line.len() as u64;
		tracing::info!(id = u.id, %item, units, "used");
		used.push(u);
		Ok(())
	}

	/// Those whose position is in `window`.
	pub(crate) fn page(&self, window: std::ops::Range<usize>) -> Vec<Used> {
		let inner = self.inner.lock().expect("never held across a panic");
		inner.used.iter().skip(window.start).take(window.len()).cloned().collect()
	}
}

struct Inner {
	file: File,
	/// Its whole lines' bytes: where a failed append is cut back to.
	len: u64,
	used: Vec<Used>,
}
