//! A gmail's inbox over IMAPS on 993 at a public address: whether it takes its credentials, and its
//! mail with Google read for [`crate::inbox`].

use std::{
	net::{IpAddr, SocketAddr},
	sync::Arc,
	time::Duration,
};

use eyre::WrapErr;
use jiff::{SignedDuration, Timestamp};
use rustls_platform_verifier::ConfigVerifierExt;
use tokio::{
	io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
	net::TcpStream,
};
use tokio_rustls::{
	TlsConnector,
	client::TlsStream,
	rustls::{ClientConfig, pki_types::ServerName},
};

use crate::{
	inbox::{Account, Batch, Cursor, Mail, Mailbox},
	store::connections::Inbox,
};

/// IMAP over TLS from the first byte; nothing else is dialled, so a host cannot point the probe
/// at another service.
pub(crate) const PORT: u16 = 993;
const TIMEOUT: Duration = Duration::from_secs(10);
/// A batch's read: the login, the search and the fetch of its messages.
const READ_WITHIN: Duration = Duration::from_secs(60);
/// What of a message is fetched: its headers and the start of its text.
const BODY_MAX: usize = 128 * 1024;
/// What a session may answer in all: a batch, and the search of a whole mailbox before it.
const ANSWERED_MAX: usize = 32 << 20;
/// Gmail's search for mail with Google: from or to any google.com address, its promotions left out, and what
/// is not sent yet.
const WITH_GOOGLE: &str = "{from:google.com to:google.com} -category:promotions -in:drafts";

/// Gmail over IMAP: its All Mail searched with Gmail's own syntax, read only.
pub struct Gmail;

/// Why the inbox did not log in, as its owner reads it; `Err` is ours.
pub(crate) async fn login(inbox: Inbox<'_>, password: &str) -> eyre::Result<Result<(), String>> {
	match tokio::time::timeout(TIMEOUT, open(inbox, password)).await {
		Ok(Ok(Ok(s))) => s.logout().await.map(Ok),
		Ok(r) => r.map(|r| r.map(drop)),
		Err(_) => Ok(Err(format!("{}:{PORT} did not answer within {}s", inbox.host, TIMEOUT.as_secs()))),
	}
}

impl Mailbox for Gmail {
	async fn read(&self, account: Account<'_>, after: Option<Cursor>, limit: usize) -> eyre::Result<Batch> {
		match tokio::time::timeout(READ_WITHIN, read(account, after, limit)).await {
			Ok(r) => r,
			Err(_) => eyre::bail!("{}:{PORT} did not finish a read within {}s", account.host, READ_WITHIN.as_secs()),
		}
	}
}

async fn read(account: Account<'_>, after: Option<Cursor>, limit: usize) -> eyre::Result<Batch> {
	let Account { host, username, password } = account;
	let mut s = open(Inbox { host, username }, password).await?.map_err(|why| eyre::eyre!(why))?;
	let all = s
		.ok(r#"LIST "" "*""#, "LIST")
		.await?
		.iter()
		.find_map(all_mail)
		.ok_or_else(|| eyre::eyre!("{host} lists no \\All mailbox"))?;
	let examined = s.ok(&format!("EXAMINE {}", quoted(&all)), "EXAMINE").await?;
	let validity: u32 = examined
		.iter()
		.find_map(|u| u.text.split_once("[UIDVALIDITY ").and_then(|(_, r)| r.split_once(']')))
		.ok_or_else(|| eyre::eyre!("{host} named no UIDVALIDITY of {all}"))?
		.0
		.parse()
		.wrap_err("its UIDVALIDITY")?;
	let from = match after {
		Some(c) if c.validity == validity => c.uid,
		_ => 0,
	};
	let mut uids = s.search(&format!("UID {}:* X-GM-RAW {}", from.max(1), quoted(WITH_GOOGLE))).await?;
	// the one before, and `n:*` past the last UID is the last one
	uids.retain(|&u| u > from);
	uids.sort_unstable();
	uids.truncate(limit);
	let mut mails = Vec::new();
	if !uids.is_empty() {
		let set = uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
		let sent = s.search(&format!(r#"UID {set} X-GM-LABELS "\\Sent""#)).await?;
		let fetched = s
			.ok(&format!("UID FETCH {set} (UID X-GM-MSGID X-GM-THRID INTERNALDATE BODY.PEEK[]<0.{BODY_MAX}>)"), "FETCH")
			.await?;
		for u in fetched.iter().filter(|u| u.text.contains("X-GM-THRID")) {
			mails.push(mail(u, &sent, Timestamp::now())?);
		}
		mails.sort_by_key(|m| m.uid);
	}
	s.logout().await?;
	Ok(Batch { validity, mails })
}

/// The inbox logged in, or why not as its owner reads it.
async fn open(inbox: Inbox<'_>, password: &str) -> eyre::Result<Result<Session, String>> {
	let Inbox { host, username } = inbox;
	for (what, v) in [("username", username), ("password", password)] {
		if v.is_empty() || !v.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
			return Ok(Err(format!("the {what} is printable ASCII, not empty")));
		}
	}
	let addrs: Vec<SocketAddr> = match tokio::net::lookup_host((host, PORT)).await {
		Ok(a) => a.collect(),
		Err(e) => return Ok(Err(format!("{host} does not resolve: {e}"))),
	};
	let Some(&addr) = addrs.first() else {
		return Ok(Err(format!("{host} resolves to no address")));
	};
	if let Some(inner) = addrs.iter().find(|a| !public(a.ip())) {
		return Ok(Err(format!("{host} resolves to {}, not a public address", inner.ip())));
	}
	let Ok(name) = ServerName::try_from(host.to_owned()) else {
		return Ok(Err(format!("{host:?} is not a host name")));
	};
	let tcp = match TcpStream::connect(addr).await {
		Ok(s) => s,
		Err(e) => return Ok(Err(format!("{addr} refused the connection: {e}"))),
	};
	let config = ClientConfig::with_platform_verifier().map_err(|e| eyre::eyre!(e).wrap_err("the platform's certificate verifier"))?;
	let tls = match TlsConnector::from(Arc::new(config)).connect(name, tcp).await {
		Ok(s) => s,
		Err(e) => return Ok(Err(format!("TLS with {host}: {e}"))),
	};
	let mut s = Session {
		io: BufReader::new(tls),
		tag: 0,
		left: ANSWERED_MAX,
	};
	let line = s.line("the greeting").await?;
	if !line.starts_with("* OK") {
		return Ok(Err(format!("{host} greeted with {:?}", line.trim_end())));
	}
	Ok(match s.run(&format!("LOGIN {} {}", quoted(username), quoted(password)), "LOGIN").await {
		Ok((_, status)) if status.starts_with("OK") => Ok(s),
		Ok((_, status)) => Err(format!("{host} refused the login: {status}")),
		Err(e) => Err(format!("{host} at LOGIN: {e}")),
	})
}

/// An IMAP quoted string; [`open`] lets in no control character, so no literal is needed.
fn quoted(s: &str) -> String {
	format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

struct Session {
	io: BufReader<TlsStream<TcpStream>>,
	tag: u32,
	/// What it may still answer, bytes.
	left: usize,
}
impl Session {
	/// `command`'s untagged responses and its tagged status; `verb` names it in errors, never its
	/// arguments (LOGIN's are the credentials).
	async fn run(&mut self, command: &str, verb: &str) -> eyre::Result<(Vec<Untagged>, String)> {
		self.tag += 1;
		let tag = format!("t{} ", self.tag);
		self.io.write_all(format!("{tag}{command}\r\n").as_bytes()).await?;
		self.io.flush().await?;
		let mut untagged = Vec::new();
		//LOOP: a response a turn until the tagged one; the caller's timeout bounds it
		loop {
			let (text, literals) = self.response(verb).await?;
			if let Some(status) = text.strip_prefix(&tag) {
				return Ok((untagged, status.to_owned()));
			}
			// a continuation is never asked for: nothing sent here is a literal
			let Some(rest) = text.strip_prefix("* ") else {
				eyre::bail!("{verb}: {text:?} is not a response");
			};
			untagged.push(Untagged { text: rest.to_owned(), literals });
		}
	}

	/// [`Self::run`], its status OK.
	async fn ok(&mut self, command: &str, verb: &str) -> eyre::Result<Vec<Untagged>> {
		let (untagged, status) = self.run(command, verb).await?;
		eyre::ensure!(status.starts_with("OK"), "{verb}: {status}");
		Ok(untagged)
	}

	/// The UIDs `criteria` finds.
	async fn search(&mut self, criteria: &str) -> eyre::Result<Vec<u32>> {
		let found = self.ok(&format!("UID SEARCH {criteria}"), "SEARCH").await?;
		found
			.iter()
			.filter_map(|u| u.text.strip_prefix("SEARCH"))
			.flat_map(str::split_whitespace)
			.map(|n| n.parse().wrap_err_with(|| format!("a UID {n:?}")))
			.collect()
	}

	async fn line(&mut self, verb: &str) -> eyre::Result<String> {
		let mut raw = Vec::new();
		let most = u64::try_from(self.left).expect("within u64");
		let n = (&mut self.io).take(most).read_until(b'\n', &mut raw).await?;
		self.answers(n, verb)?;
		eyre::ensure!(raw.ends_with(b"\n"), "the connection closed at {verb}, or answers more than {ANSWERED_MAX} bytes");
		Ok(String::from_utf8_lossy(&raw).into_owned())
	}

	fn answers(&mut self, n: usize, verb: &str) -> eyre::Result<()> {
		self.left = self.left.checked_sub(n).ok_or_else(|| eyre::eyre!("{verb}: an answer past {ANSWERED_MAX} bytes"))?;
		Ok(())
	}

	/// A response's lines and the literals between them.
	async fn response(&mut self, verb: &str) -> eyre::Result<(String, Vec<Vec<u8>>)> {
		let (mut text, mut literals) = (String::new(), Vec::new());
		//LOOP: a line a turn, a literal after each that ends in one
		loop {
			let line = self.line(verb).await?;
			let line = line.trim_end_matches(['\r', '\n']);
			let literal = line
				.strip_suffix('}')
				.and_then(|l| l.rsplit_once('{'))
				.filter(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
			let Some((before, n)) = literal else {
				text.push_str(line);
				return Ok((text, literals));
			};
			let n: usize = n.parse().wrap_err("a literal's length")?;
			eyre::ensure!(n <= BODY_MAX, "{verb}: a literal of {n} bytes");
			self.answers(n, verb)?;
			let mut bytes = vec![0; n];
			self.io.read_exact(&mut bytes).await?;
			text.push_str(before);
			text.push_str("{}");
			literals.push(bytes);
		}
	}

	/// Said, not waited on: the connection is let go either way.
	async fn logout(mut self) -> eyre::Result<()> {
		self.io.write_all(b"z LOGOUT\r\n").await?;
		self.io.flush().await?;
		Ok(())
	}
}

/// An untagged response: its text, each literal in it as `{}`, and the literals in order.
struct Untagged {
	text: String,
	literals: Vec<Vec<u8>>,
}

/// The mailbox a LIST response flags `\\All`: Gmail's All Mail, whatever its language names it.
fn all_mail(u: &Untagged) -> Option<String> {
	let (flags, rest) = u.text.strip_prefix("LIST (")?.split_once(')')?;
	if !flags.split_whitespace().any(|f| f.eq_ignore_ascii_case("\\All")) {
		return None;
	}
	let rest = rest.trim_end();
	if rest.ends_with("{}") {
		return Some(String::from_utf8_lossy(u.literals.last()?).into_owned());
	}
	Some(rest.strip_suffix('"')?.rsplit_once(" \"")?.1.replace("\\\"", "\"").replace("\\\\", "\\"))
}

/// A message of a FETCH response, `sent` the UIDs of those the account sent: a From alone anyone can write.
fn mail(u: &Untagged, sent: &[u32], now: Timestamp) -> eyre::Result<Mail> {
	let text = &u.text;
	let number = |key: &str| -> eyre::Result<u64> {
		let mut words = text.split([' ', '(', ')']);
		words.by_ref().find(|w| *w == key).ok_or_else(|| eyre::eyre!("a FETCH without {key}: {text}"))?;
		words
			.next()
			.ok_or_else(|| eyre::eyre!("{key} ends a FETCH: {text}"))?
			.parse()
			.wrap_err_with(|| format!("{key} of {text}"))
	};
	let uid = u32::try_from(number("UID")?).wrap_err("a UID is 32 bits")?;
	let received = text
		.split_once("INTERNALDATE \"")
		.and_then(|(_, r)| r.split_once('"'))
		.ok_or_else(|| eyre::eyre!("a FETCH without INTERNALDATE: {text}"))?
		.0;
	let date = jiff::fmt::strtime::parse("%d-%b-%Y %H:%M:%S %z", received.trim_start())
		.and_then(|t| t.to_timestamp())
		.wrap_err_with(|| format!("INTERNALDATE {received:?}"))?;
	eyre::ensure!(date.duration_since(now) < SignedDuration::from_hours(24), "message {uid} is of {date}, which is yet to come");
	let [body] = u.literals.as_slice() else {
		eyre::bail!("a FETCH of {uid} with {} literals", u.literals.len());
	};
	let parsed = mail_parser::MessageParser::default()
		.parse(body.as_slice())
		.ok_or_else(|| eyre::eyre!("message {uid} does not parse"))?;
	let named = |a: mail_parser::Mailbox<'_>| match (a.name(), a.address()) {
		(Some(n), Some(e)) => Some(format!("{n} <{e}>")),
		(None, Some(e)) => Some(e.to_owned()),
		(Some(n), None) => Some(n.to_owned()),
		(None, None) => None,
	};
	let from = parsed.from().and_then(|a| a.first());
	let sent = sent.contains(&uid);
	let counterparts = match (sent, parsed.to()) {
		(true, Some(to)) => to.mailboxes().filter_map(named).collect(),
		// a message to nobody named: undisclosed recipients
		(true, None) => Vec::new(),
		(false, _) => from.and_then(named).into_iter().collect(),
	};
	let froms: Vec<&str> = parsed.all_from().flat_map(|a| a.mailboxes().filter_map(|m| m.address()).collect::<Vec<_>>()).collect();
	let verdicts: Vec<String> = parsed
		.headers()
		.all("Authentication-Results")
		.map(|h| String::from_utf8_lossy(h.raw_value()).into_owned())
		.collect();
	let whole = body.len() < BODY_MAX;
	let mut text = parsed.body_text(0).map(|t| t.into_owned()).unwrap_or_default(); // no text part: none to read
	if sent {
		if !whole {
			text = format!("[its files past those named above were not read]\n{text}");
		}
		// what we sent along: Google's answers speak of it
		for name in parsed.attachments().filter_map(|a| a.attachment_name().map(str::to_owned)) {
			text = format!("[attached: {name}]\n{text}");
		}
	}
	Ok(Mail {
		uid,
		message: number("X-GM-MSGID")?,
		thread: number("X-GM-THRID")?,
		date,
		subject: parsed.subject().map(str::to_owned),
		counterparts,
		from: from.and_then(|a| a.address()).map(str::to_owned),
		reply_to: parsed.reply_to().and_then(|a| a.first()).and_then(|a| a.address()).map(str::to_owned),
		// neither: undisclosed recipients
		to: [parsed.to(), parsed.cc()]
			.into_iter()
			.flatten()
			.flat_map(|a| a.mailboxes().filter_map(|m| m.address()).map(str::to_owned).collect::<Vec<_>>())
			.collect(),
		message_id: parsed.message_id().map(str::to_owned),
		refs: parsed
			.references()
			.into_iter()
			.chain(parsed.in_reply_to())
			.flat_map(|l| l.iter().map(str::to_owned).collect::<Vec<_>>())
			.collect(),
		authentic: sa_core::mail::authentic(&froms, &verdicts.iter().map(String::as_str).collect::<Vec<_>>()),
		sent,
		text,
		whole,
	})
}

pub(crate) fn public(ip: IpAddr) -> bool {
	match ip {
		IpAddr::V4(v4) => {
			let [a, b, ..] = v4.octets();
			!(v4.is_private()
				|| v4.is_loopback()
				|| v4.is_link_local()
				|| v4.is_unspecified()
				|| v4.is_broadcast()
				|| v4.is_documentation()
				|| v4.is_multicast()
				|| a == 0 || (a == 100 && (64..128).contains(&b)))
		}
		IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
			Some(v4) => public(IpAddr::V4(v4)),
			None => {
				let first = v6.segments()[0];
				!(v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80)
			}
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn fetched(date: &str, body: &str) -> Untagged {
		Untagged {
			text: format!("1 FETCH (UID 7 X-GM-MSGID 1 X-GM-THRID 2 INTERNALDATE \"{date}\" BODY[]<0> {{}})"),
			literals: vec![body.as_bytes().to_vec()],
		}
	}

	#[test]
	fn a_message_is_of_a_day_that_was_and_says_whether_it_was_read_to_its_end() {
		let now: Timestamp = "2026-10-01T09:00:00Z".parse().unwrap();
		let head = "From: a@gmail.com\r\nTo: b@x.com\r\nCc: c@google.com\r\n";
		let read = |date, body: &str| mail(&fetched(date, body), &[7], now);
		let whole = read("01-Oct-2026 08:00:00 +0000", &format!("{head}\r\nHello")).unwrap();
		assert_eq!((whole.to, whole.whole, whole.text.trim()), (vec!["b@x.com".to_owned(), "c@google.com".to_owned()], true, "Hello"));
		for date in ["30-Dec-9999 00:00:00 +0000", "03-Oct-2026 09:00:00 +0000"] {
			assert!(read(date, &format!("{head}\r\nHello")).is_err(), "{date}");
		}
		let cut = read("01-Oct-2026 08:00:00 +0000", &format!("{head}\r\n{}", "x".repeat(BODY_MAX - head.len() - 2))).unwrap();
		assert!(!cut.whole && cut.text.contains("were not read"), "what it says past there, and its files, are not known");
	}
}
