//! The networks alerts go out through, each with its configuration or refusing.

use sa_core::{
	alerts::Method,
	notify::{ChatId, Failure, Rendered},
};
use sa_panel::{
	alerts::{Address, Message, Outlets},
	telegram::{Bot, CallbackId, ChatInfo, InlineButton, Target, callbacks},
};

use crate::telegram::BotApi;

#[derive(Clone, Debug, Default)]
pub struct Networks {
	pub telegram: Option<BotApi>,
}

impl Networks {
	/// The methods a destination can be added by here.
	pub fn methods(&self) -> Vec<Method> {
		[self.telegram.is_some().then_some(Method::Telegram)].into_iter().flatten().collect()
	}

	fn bot(&self) -> Result<&BotApi, Failure> {
		self.telegram.as_ref().ok_or_else(|| unconfigured(Method::Telegram))
	}
}

impl Outlets for Networks {
	async fn send_to(&self, to: &Address, message: &Message) -> Result<String, Failure> {
		match to {
			Address::Telegram(chat) => {
				let mut buttons = callbacks(message.buttons.clone());
				buttons.extend(message.links.iter().cloned().map(|(label, url)| InlineButton::Url { label, url }));
				self.bot()?.send(*chat, &message.text, &buttons, message.photo.clone()).await.map(|id| id.to_string())
			}
			other => Err(unconfigured(other.method())),
		}
	}
}

impl Bot for Networks {
	async fn send(&self, to: Target, message: &Rendered, buttons: &[InlineButton], photo: Option<Vec<u8>>) -> Result<i64, Failure> {
		self.bot()?.send(to, message, buttons, photo).await
	}

	async fn edit(&self, chat: ChatId, message_id: i64, message: &Rendered, buttons: &[InlineButton]) -> Result<(), Failure> {
		self.bot()?.edit(chat, message_id, message, buttons).await
	}

	async fn answer(&self, callback: &CallbackId, text: &str) -> Result<(), Failure> {
		self.bot()?.answer(callback, text).await
	}

	async fn chat(&self, chat: &str) -> Result<ChatInfo, Failure> {
		self.bot()?.chat(chat).await
	}

	async fn is_admin(&self, chat: ChatId, user_id: i64) -> Result<bool, Failure> {
		self.bot()?.is_admin(chat, user_id).await
	}
}

fn unconfigured(method: Method) -> Failure {
	Failure::Refused(format!("{} is not configured on this server", method.as_ref()))
}
