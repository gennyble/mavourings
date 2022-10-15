use std::{collections::HashMap, fmt, io, path::Path, str::FromStr, time::Duration};

use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rand::{rngs::OsRng, Rng};
use tokio::{io::AsyncWriteExt, sync::RwLock};

const BASE58: &'static [u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const USER_ID_LENGTH: usize = 6;
const SESSION_ID_LENGTH: usize = 6;

/// Random Base58 string, `count` characters long, using OsRng which is assumed
/// to be secure
/// > assumed that system always provides high-quality cryptographically secure random data
pub fn random_base58(count: usize) -> String {
	let mut rng = OsRng::default();
	std::iter::from_fn(|| Some(BASE58[rng.gen_range(0..BASE58.len())] as char))
		.take(count)
		.collect()
}

#[derive(Debug)]
pub struct Users {
	pub(crate) users: RwLock<HashMap<UserId, UserEntry>>,
}

impl Users {
	pub fn new() -> Users {
		Self {
			users: RwLock::new(HashMap::new()),
		}
	}

	/// Registers a User, saving their details in memory and returning the value
	/// to use with a Set-Cookie heder to create the session on the client.
	pub async fn register(
		&self,
		email: Option<String>,
		username: String,
		password: String,
	) -> Session {
		let mut entry = UserEntry::new_user(email, username, password);
		let session = entry.new_session();

		//FIXME: gen- we should check here that the UserId is unique. The
		//  chances are low but let's not loose data
		{
			let mut users = self.users.write().await;
			users.insert(entry.id.clone(), entry);
		}

		session
	}

	/// Login a user. We find their [UserEntry] by looking for their username
	/// and then verify their password. Returns a [Session]
	pub async fn login(&self, username: String, password: String) -> Option<Session> {
		let mut lock = self.users.write().await;

		let entry = lock.values_mut().find(|entry| entry.username == username);

		match entry {
			None => None,
			Some(entry) => {
				if entry.verify_password(password) {
					Some(entry.new_session())
				} else {
					None
				}
			}
		}
	}

	/// Login a user. We find their [UserEntry] by looking for their username
	/// and then verify their password. Returns an `Option<[UserStub]>` which
	/// will only be filled if a user was found and their password verified.
	pub async fn authenticate(&self, username: String, password: String) -> Option<UserStub> {
		let mut lock = self.users.write().await;

		let entry = lock.values_mut().find(|entry| entry.username == username);

		match entry {
			None => None,
			Some(entry) => {
				if entry.verify_password(password) {
					Some(entry.stub())
				} else {
					None
				}
			}
		}
	}

	/// Remove the provided [SessionId] from the session list and return a [UserStub]
	/// if a user was found with that session ID.
	pub async fn logout(&self, sid: SessionId) -> Option<UserStub> {
		let mut lock = self.users.write().await;

		for user in lock.values_mut() {
			match user.sessions.iter().position(|v| *v == sid) {
				Some(idx) => {
					user.sessions.remove(idx);
					return Some(user.stub());
				}
				None => (),
			}
		}

		None
	}

	/// Searches for a user by an assocaited [SessionId], returning a [Session] if a user is found and `None` otherwise
	pub async fn session_by_id(&self, sid: SessionId) -> Option<Session> {
		{
			let lock = self.users.read().await;

			for user in lock.values() {
				if user.sessions.contains(&sid) {
					return Some(Session {
						stub: user.stub(),
						sid,
					});
				}
			}
		}

		None
	}

	/// Searches for a user by an assocaited [SessionId], returning a [UserStub] if a user is found and `None` otherwise
	pub async fn stub_by_session(&self, sid: SessionId) -> Option<UserStub> {
		{
			let lock = self.users.read().await;

			for user in lock.values() {
				if user.sessions.contains(&sid) {
					return Some(user.stub());
				}
			}
		}

		None
	}

	pub async fn stub_by_uid(&self, uid: UserId) -> Option<UserStub> {
		self.users.read().await.get(&uid).map(|u| u.stub())
	}

	/// Searches for users by their username, returning a `Vec<[UserStub]>` containing any found users
	pub async fn stub_by_username<S: AsRef<str>>(&self, username: S) -> Vec<UserStub> {
		let username = username.as_ref();
		let mut matches = vec![];

		{
			let lock = self.users.read().await;

			for user in lock.values() {
				if user.username == username {
					matches.push(user.stub())
				}
			}
		}

		matches
	}

	pub async fn save<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
		let mut buf = String::new();
		{
			let lock = self.users.read().await;
			for entry in lock.values() {
				buf.push_str(&format!("{}\n", entry));
			}
		}

		let mut file = tokio::fs::File::create(path).await?;
		file.write_all(buf.as_bytes()).await?;
		file.flush().await
	}

	pub async fn load<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
		let string = tokio::fs::read_to_string(path).await?;

		{
			let mut lock = self.users.write().await;
			for line in string.lines() {
				let entry = UserEntry::from_str(line).unwrap();
				lock.insert(entry.id.clone(), entry);
			}
		}

		Ok(())
	}
}

/// Information about a user. Returned by [UserEntry::register] and [UserEnry::login].
pub struct UserStub {
	pub email: Option<String>,
	pub id: UserId,
	pub username: String,
}

pub struct Session {
	pub stub: UserStub,
	pub sid: SessionId,
}

impl Session {
	pub fn login_cookie(&self) -> String {
		session_set_cookie(&self.sid)
	}

	pub fn logout_cookie(&self) -> String {
		session_clear_cookie(&self.sid)
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct UserId(String);

impl UserId {
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl fmt::Display for UserId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.0)
	}
}

impl From<String> for UserId {
	fn from(s: String) -> Self {
		Self(s)
	}
}

#[derive(Clone, Debug, PartialEq)]
pub struct UserEntry {
	pub id: UserId,
	pub email: Option<String>,
	pub username: String,
	pub password_hash: String,
	sessions: Vec<SessionId>,
}

impl UserEntry {
	/// Make a new user, allocating an new UserId and hashing their password
	pub fn new_user(email: Option<String>, username: String, password_raw: String) -> UserEntry {
		let password_hash = Self::hash_password(password_raw);
		let id = Self::generate_user_id();

		Self {
			id,
			email,
			username,
			password_hash,
			sessions: vec![],
		}
	}

	pub fn new_session(&mut self) -> Session {
		let sid = Self::generate_session_id();
		self.sessions.push(sid.clone());

		Session {
			stub: self.stub(),
			sid,
		}
	}

	/// Make a [UserStub] with the provided [SessionId]
	pub fn stub(&self) -> UserStub {
		UserStub {
			email: self.email.clone(),
			id: self.id.clone(),
			username: self.username.clone(),
		}
	}

	/// Hash a password with [Argon2]
	fn hash_password(password: String) -> String {
		Argon2::default()
			.hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
			.unwrap()
			.to_string()
	}

	fn verify_password(&self, password: String) -> bool {
		let parsed_hash = PasswordHash::new(&self.password_hash).unwrap();

		Argon2::default()
			.verify_password(password.as_bytes(), &parsed_hash)
			.is_ok()
	}

	/// Get a new [UserId]
	fn generate_user_id() -> UserId {
		UserId(random_base58(USER_ID_LENGTH))
	}

	/// Get a new [SessionId]
	fn generate_session_id() -> SessionId {
		SessionId(random_base58(SESSION_ID_LENGTH))
	}
}

impl fmt::Display for UserEntry {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{} ", self.id)?;

		//FIXME: this disallows <> in emails which is incorrect behavior
		match self.email.as_ref() {
			Some(email) => write!(f, "<{email}> ")?,
			None => write!(f, "<> ")?,
		}

		write!(f, "{} ", self.username)?;
		write!(f, "{} ", self.password_hash)?;

		let mut session_str = String::new();
		for session in &self.sessions {
			session_str.push_str(session.as_str());
			session_str.push(',');
		}

		write!(f, "sessions={}", session_str)
	}
}

impl FromStr for UserEntry {
	//FIXME: gen- can we have real errors please?
	type Err = ();

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let (id, s) = match s.split_once(" ") {
			Some((id, s)) => (UserId(id.to_string()), s),
			None => return Err(()),
		};

		if !s.starts_with('<') {
			return Err(());
		}
		let s = &s[1..];

		let (email, s) = match s.find('>') {
			None => return Err(()),
			Some(idx) => {
				let email = &s[..idx];
				// One for the > and one for the space
				let s = &s[idx + 2..];

				if email.is_empty() {
					(None, s)
				} else {
					(Some(email.to_string()), s)
				}
			}
		};

		let mut splits = s.split(' ');
		let username = splits.next().unwrap().to_string();
		let password_hash = splits.next().unwrap().to_string();
		let session_str = splits.next().unwrap();

		let sessions = match session_str.strip_prefix("sessions=") {
			None => return Err(()),
			Some(sessions) => sessions
				.split(',')
				.filter_map(|sid| {
					if sid.is_empty() {
						None
					} else {
						Some(SessionId(sid.to_string()))
					}
				})
				.collect(),
		};

		Ok(Self {
			id,
			email,
			username,
			password_hash,
			sessions,
		})
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SessionId(String);

impl SessionId {
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl fmt::Display for SessionId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.0)
	}
}

impl From<String> for SessionId {
	fn from(s: String) -> Self {
		Self(s)
	}
}

/// Get the value bit of a Set-Cookie header to create a session
fn session_set_cookie(sid: &SessionId) -> String {
	crate::cookie::SetCookie::new("sid".into(), sid.to_string())
		.secure(true)
		.httponly(true)
		.max_age(Some(Duration::from_secs(60 * 60 * 24 * 30)))
		.path(Some(String::from("/")))
		.as_string()
}

/// Get the value bit of a Set-Cookie header to clear a session
pub fn session_clear_cookie(sid: &SessionId) -> String {
	crate::cookie::SetCookie::new("sid".into(), sid.to_string())
		.secure(true)
		.httponly(true)
		.max_age(Some(Duration::from_secs(0)))
		.path(Some(String::from("/")))
		.as_string()
}

#[cfg(test)]
mod tests {
	use super::UserEntry;

	fn check_entry_saveload(entry: UserEntry) {
		let entry_string = entry.to_string();
		let parsed_entry: UserEntry = entry_string.parse().unwrap();

		assert_eq!(entry, parsed_entry);
	}

	#[test]
	fn userentry_save_load() {
		let entry = UserEntry::new_user(Some("test".into()), "gen".into(), "password".into());
		check_entry_saveload(entry);

		let entry_no_email = UserEntry::new_user(None, "gen".into(), "password".into());
		check_entry_saveload(entry_no_email);

		let mut entry_with_sessions =
			UserEntry::new_user(Some("test".into()), "gen".into(), "password".into());
		entry_with_sessions.new_session();
		entry_with_sessions.new_session();
		check_entry_saveload(entry_with_sessions);
	}
}
