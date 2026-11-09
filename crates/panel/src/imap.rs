//! Whether an inbox takes its credentials: IMAPS on 993 at a public address, greeting, `LOGIN`,
//! `LOGOUT`. Nothing here reads mail.

use std::{
	net::{IpAddr, SocketAddr},
	sync::Arc,
	time::Duration,
};

use rustls_platform_verifier::ConfigVerifierExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_rustls::{
	TlsConnector,
	rustls::{ClientConfig, pki_types::ServerName},
};

/// IMAP over TLS from the first byte; nothing else is dialled, so a host cannot point the probe
/// at another service.
const PORT: u16 = 993;
const TIMEOUT: Duration = Duration::from_secs(10);

/// Why the inbox did not log in, as its owner reads it; `Err` is ours.
pub(crate) async fn login(host: &str, username: &str, password: &str) -> eyre::Result<Result<(), String>> {
	for (what, v) in [("username", username), ("password", password)] {
		if v.is_empty() || !v.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
			return Ok(Err(format!("the {what} is printable ASCII, not empty")));
		}
	}
	match tokio::time::timeout(TIMEOUT, exchange(host, username, password)).await {
		Ok(r) => r,
		Err(_) => Ok(Err(format!("{host}:{PORT} did not answer within {}s", TIMEOUT.as_secs()))),
	}
}

async fn exchange(host: &str, username: &str, password: &str) -> eyre::Result<Result<(), String>> {
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
	let tcp = match tokio::net::TcpStream::connect(addr).await {
		Ok(s) => s,
		Err(e) => return Ok(Err(format!("{addr} refused the connection: {e}"))),
	};
	let config = ClientConfig::with_platform_verifier().map_err(|e| eyre::eyre!(e).wrap_err("the platform's certificate verifier"))?;
	let tls = match TlsConnector::from(Arc::new(config)).connect(name, tcp).await {
		Ok(s) => s,
		Err(e) => return Ok(Err(format!("TLS with {host}: {e}"))),
	};
	let mut io = BufReader::new(tls);
	let mut line = String::new();
	io.read_line(&mut line).await?;
	if !line.starts_with("* OK") {
		return Ok(Err(format!("{host} greeted with {:?}", line.trim_end())));
	}
	io.write_all(format!("a LOGIN {} {}\r\n", quoted(username), quoted(password)).as_bytes()).await?;
	io.flush().await?;
	let answer = loop {
		line.clear();
		if io.read_line(&mut line).await? == 0 {
			return Ok(Err(format!("{host} closed the connection at LOGIN")));
		}
		if let Some(rest) = line.strip_prefix("a ") {
			break rest.trim_end().to_owned();
		}
	};
	if !answer.starts_with("OK") {
		return Ok(Err(format!("{host} refused the login: {answer}")));
	}
	io.write_all(b"b LOGOUT\r\n").await?;
	io.flush().await?;
	Ok(Ok(()))
}

/// An IMAP quoted string; [`login`] lets in no control character, so no literal is needed.
fn quoted(s: &str) -> String {
	format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn public(ip: IpAddr) -> bool {
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
