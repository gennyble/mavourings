#[cfg(feature = "cookie")]
pub mod cookie;

pub mod query;

#[cfg(feature = "send_file")]
pub async fn file_string_reply<P: AsRef<std::path::Path>>(
	path: P,
) -> Result<hyper::Response<hyper::Body>, Box<dyn std::error::Error>> {
	use hyper::{Body, Response};
	use mime_guess::MimeGuess;
	use std::error::Error;

	let file = tokio::fs::read_to_string(path.as_ref()).await?;

	let mut resp = Response::builder().status(200);

	if let Some(guess) = MimeGuess::from_path(path).first() {
		resp = resp.header("content-type", guess.to_string());
	};

	resp.body(Body::from(file))
		.map_err(|e| Box::new(e) as Box<dyn Error>)
}

#[cfg(feature = "template")]
pub mod template {
	use std::{error::Error, str::FromStr};

	use bempline::Document;
	use hyper::{Body, Response};
	use mime_guess::{Mime, MimeGuess};

	pub struct Template {
		document: Document,
		guess: Option<Mime>,
	}

	impl Template {
		pub async fn file<P: AsRef<std::path::Path>>(path: P) -> Template {
			//TODO: gen- remove unwrap
			let file = tokio::fs::read_to_string(path.as_ref()).await.unwrap();

			let document = Document::from_str(&file).unwrap();

			let guess = MimeGuess::from_path(path).first();

			Self { document, guess }
		}

		pub fn set<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) {
			self.document.set(key, value)
		}

		pub fn as_response(self) -> Result<Response<Body>, Box<dyn Error>> {
			let mut resp = Response::builder().status(200);

			if let Some(guess) = self.guess {
				resp = resp.header("content-type", guess.to_string());
			};

			resp.body(Body::from(self.document.compile()))
				.map_err(|e| Box::new(e) as Box<dyn Error>)
		}
	}
}
