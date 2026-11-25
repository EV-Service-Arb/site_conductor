//! A reply sent as its gmail, over SMTPS on smtp.gmail.com, for [`crate::mail`].

use std::time::Duration;

use lettre::{
	AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
	message::{Attachment, MultiPart, SinglePart, header::ContentType},
	transport::smtp::{self, authentication::Credentials},
};

use crate::{
	inbox::Account,
	mail::{Outgoing, SendError, Sending, Smtp},
};

const HOST: &str = "smtp.gmail.com";
pub(crate) const PORT: u16 = lettre::transport::smtp::SUBMISSIONS_PORT;
const TIMEOUT: Duration = Duration::from_secs(60);

/// Gmail over SMTP.
#[derive(Debug)]
pub struct Gmail;

impl Smtp for Gmail {
	fn send<'a>(&'a self, account: Account<'a>, mail: &'a Outgoing) -> Sending<'a> {
		Box::pin(send(account, mail))
	}
}

async fn send(account: Account<'_>, mail: &Outgoing) -> Result<(), SendError> {
	let message = built(account.username, mail).map_err(|e| SendError::NotSent(format!("{e:#}")))?;
	let gmail = AsyncSmtpTransport::<Tokio1Executor>::relay(HOST)
		.map_err(|e| SendError::NotSent(e.to_string()))?
		.credentials(Credentials::new(account.username.to_owned(), account.password.to_owned()))
		.timeout(Some(TIMEOUT))
		.build();
	match gmail.test_connection().await {
		Ok(true) => {}
		Ok(false) => return Err(SendError::NotSent(format!("{HOST} does not answer"))),
		Err(e) => return Err(failed(&e, SendError::NotSent(format!("{HOST}: {e}")))),
	}
	match gmail.send(message).await {
		Ok(_) => Ok(()),
		Err(e) if e.is_permanent() || e.is_transient() => Err(failed(&e, SendError::NotSent(format!("{HOST} refused it: {e}")))),
		Err(e) => Err(failed(&e, SendError::Unknown(format!("{HOST}: {e}")))),
	}
}

/// `otherwise`, unless it is the login that is refused.
fn failed(e: &smtp::Error, otherwise: SendError) -> SendError {
	match e.status().map(u16::from) {
		Some(534 | 535) => SendError::Login(format!("{HOST}: {e}")),
		_ => otherwise,
	}
}

fn built(from: &str, mail: &Outgoing) -> eyre::Result<Message> {
	let bracketed = |ids: &[String]| ids.iter().map(|i| format!("<{i}>")).collect::<Vec<_>>().join(" ");
	let mut message = Message::builder()
		.from(from.parse()?)
		.to(mail.to.parse()?)
		.subject(&mail.subject)
		.message_id(Some(format!("<{}>", mail.message_id)));
	if let Some(id) = &mail.in_reply_to {
		message = message.in_reply_to(format!("<{id}>"));
	}
	if !mail.references.is_empty() {
		message = message.references(bracketed(&mail.references));
	}
	let body = SinglePart::plain(mail.body.clone());
	if mail.files.is_empty() {
		return Ok(message.singlepart(body)?);
	}
	let mut parts = MultiPart::mixed().singlepart(body);
	for f in &mail.files {
		parts = parts.singlepart(Attachment::new(f.name.clone()).body(f.bytes.clone(), ContentType::parse(&f.kind)?));
	}
	Ok(message.multipart(parts)?)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mail::File;

	#[test]
	fn a_reply_is_threaded_plain_and_carries_its_files() {
		let mut mail = Outgoing {
			to: "googlebusinessprofile-support@google.com".into(),
			subject: "Re: [1-1234000056789] Your Request".into(),
			body: "La vidéo ne convient pas.\n".into(),
			message_id: "a@mail.gmail.com".into(),
			in_reply_to: Some("g2@mail.gmail.com".into()),
			references: vec!["g1@mail.gmail.com".into(), "g2@mail.gmail.com".into()],
			files: vec![],
		};
		let raw = |m: &Outgoing| String::from_utf8(built("jean@gmail.com", m).unwrap().formatted()).unwrap();
		let plain = raw(&mail);
		for line in [
			"From: jean@gmail.com\r\n",
			"To: googlebusinessprofile-support@google.com\r\n",
			"Message-ID: <a@mail.gmail.com>\r\n",
			"In-Reply-To: <g2@mail.gmail.com>\r\n",
			"References: <g1@mail.gmail.com> <g2@mail.gmail.com>\r\n",
			"Content-Type: text/plain; charset=utf-8\r\n",
		] {
			assert!(plain.contains(line), "{line:?} in {plain}");
		}
		mail.files.push(File {
			name: "sign.jpg".into(),
			kind: "image/jpeg".into(),
			bytes: vec![1, 2, 3],
		});
		assert!(raw(&mail).contains("filename=\"sign.jpg\""));
		mail.files[0].kind = "not a type".into();
		assert!(built("jean@gmail.com", &mail).is_err());
	}
}
