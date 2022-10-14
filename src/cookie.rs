#![allow(dead_code)]
use std::{collections::HashMap, time::Duration};

use time::{macros::format_description, PrimitiveDateTime};

pub struct SetCookie {
	key: String,
	value: String,
	expiration: Option<PrimitiveDateTime>,
	max_age: Option<Duration>,
	secure: bool,
	httponly: bool,
	path: Option<String>,
}

impl SetCookie {
	pub fn new(key: String, value: String) -> Self {
		Self {
			key,
			value,
			expiration: None,
			max_age: None,
			secure: true,
			httponly: true,
			path: None,
		}
	}

	pub fn secure(mut self, flag: bool) -> Self {
		self.secure = flag;
		self
	}

	pub fn httponly(mut self, flag: bool) -> Self {
		self.httponly = flag;
		self
	}

	pub fn max_age(mut self, seconds: Option<Duration>) -> Self {
		self.max_age = seconds;
		self
	}

	pub fn path(mut self, path: Option<String>) -> Self {
		self.path = path;
		self
	}

	pub fn as_string(&self) -> String {
		let mut cookie = format!("{}={}", self.key, self.value);

		if let Some(expiration) = self.expiration {
			let format = format_description!("[weekday repr:short], [day] [month repr:short] [year] [hour repr:24]:[minute]:[second] GMT");
			cookie.push_str(&format!("; Expires={}", expiration.format(format).unwrap()))
		}

		if let Some(duration) = self.max_age {
			cookie.push_str(&format!("; Max-Age={}", duration.as_secs()));
		}

		if self.secure {
			cookie.push_str("; Secure");
		}

		if self.httponly {
			cookie.push_str("; HttpOnly")
		}

		if let Some(path) = &self.path {
			cookie.push_str(&format!("; Path={path}"))
		}

		cookie
	}
}

pub fn parse_header(string: &str) -> Result<HashMap<&str, &str>, ()> {
	let mut cookies = HashMap::new();

	for pair in string.split(";") {
		match pair.split_once("=") {
			None => return Err(()),
			Some((key, value)) => {
				cookies.insert(key.trim(), value.trim());
			}
		}
	}

	Ok(cookies)
}
