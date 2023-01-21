use std::sync::Arc;

use async_trait::async_trait;
use axum::{
	extract::FromRequestParts,
	http::request::Parts,
	response::{IntoResponse, Response},
	Extension, RequestPartsExt,
};
use hyper::header;

use crate::users::{SessionId, Users};

#[cfg(all(feature = "users", feature = "cookie"))]
#[async_trait]
impl<S> FromRequestParts<S> for crate::users::SessionId
where
	S: Send + Sync,
{
	type Rejection = ();

	async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
		let cookie = crate::cookie::parse_header(
			parts
				.headers
				.get(header::COOKIE)
				.ok_or(())?
				.to_str()
				.map_err(|_| ())?,
		)
		.map_err(|_| ())?;

		Ok(Self(cookie.get("sid").ok_or(())?.to_string()))
	}
}

#[cfg(all(feature = "users", feature = "cookie"))]
#[async_trait]
impl<S> FromRequestParts<S> for crate::users::Session
where
	S: Send + Sync,
{
	type Rejection = Response;

	async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
		let Extension(sid) = parts
			.extract::<Extension<SessionId>>()
			.await
			.map_err(|err| err.into_response())?;

		let users: Option<&Extension<Arc<Users>>> = parts.extensions.get();

		match users {
			None => panic!(),
			Some(Extension(users)) => Ok(users.session_by_id(sid).await.unwrap()),
		}
	}
}
