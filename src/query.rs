use std::{iter::Peekable, str::Chars, vec::IntoIter};

use thiserror::Error;

///blah
#[derive(Debug)]
pub struct Query {
    parameters: Vec<Parameter>,
}

impl Query {
    /// Returns true if the query has a parameter with the given key, whether it's a bool or key-value pair.
    ///
    /// # Examples
    ///
    ///```rust
    ///use small_http::Query;
    ///let query: Query = "key=value&boolean".parse().unwrap();
    ///
    ///assert!(query.has("key"));
    ///assert!(query.has("boolean"));
    ///assert!(!query.has("notakey"));
    ///```
    pub fn has<S: AsRef<str>>(&self, key: S) -> bool {
        for param in &self.parameters {
            match param {
                Parameter::Value(param_key, _) if param_key == key.as_ref() => return true,
                Parameter::Bool(param_key) if param_key == key.as_ref() => return true,
                _ => continue,
            }
        }

        false
    }

    /// Returns true if the query has a key-value pair with the given key.
    ///
    /// # Examples
    ///
    ///```rust
    /// use small_http::Query;
    ///
    /// let query: Query = "key=value&boolean".parse().unwrap();
    ///
    /// assert!(query.has_value("key"));
    /// assert!(!query.has_value("boolean"));
    ///```
    pub fn has_value<S: AsRef<str>>(&self, key: S) -> bool {
        for param in &self.parameters {
            match param {
                Parameter::Value(param_key, _) if param_key == key.as_ref() => return true,

                _ => continue,
            }
        }

        false
    }

    /// Returns true if the query has a bool with the given name.
    ///
    /// # Examples
    ///
    ///```rust
    /// use small_http::Query;
    ///
    /// let query: Query = "key=value&boolean".parse().unwrap();
    ///
    /// assert!(query.has_bool("boolean"));
    /// assert!(!query.has_bool("key"));
    ///```
    pub fn has_bool<S: AsRef<str>>(&self, name: S) -> bool {
        for param in &self.parameters {
            match param {
                Parameter::Bool(param_key) if param_key == name.as_ref() => return true,
                _ => continue,
            }
        }

        false
    }

    /// Returns the first value from a key-value pair if one is found. If none
    /// is found, None is returned.
    ///
    /// # Examples
    ///
    ///```rust
    /// use small_http::Query;
    ///
    /// let query: Query = "key=value&boolean".parse().unwrap();
    ///
    /// assert_eq!(query.get_first_value("key"), Some("value"));
    /// assert_eq!(query.get_first_value("boolean"), None);
    ///```
    pub fn get_first_value<S: AsRef<str>>(&self, search: S) -> Option<&str> {
        for param in &self.parameters {
            match param {
                Parameter::Value(key, value) if key == search.as_ref() => return Some(value),
                _ => continue,
            }
        }

        None
    }

    /// Processes a string, converting any percent encoded characteres into
    /// their proper representations.
    ///
    /// If the second parameter is true, this function will also turn any '+'
    /// into spaces as most browsers replace spaces with plus. This will not be
    /// done if the plus is percent encoded (%2B)
    ///
    /// # Returns
    ///
    /// The decoded String on success or a QueryParseError if the decode resulted
    /// in invalid UTF8
    ///
    /// # Examples
    ///
    ///```rust
    ///use small_http::{Query, QueryParseError};
    ///
    ///assert_eq!(Query::url_decode("a+space+two%20ways%21", true), Ok(String::from("a space two ways!")));
    ///assert_eq!(Query::url_decode("invalid%1Z", true), Ok(String::from("invalid%1Z")));
    ///assert_eq!(Query::url_decode("a%20plus+sign", false), Ok(String::from("a plus+sign")));
    ///```
    pub fn url_decode<S: AsRef<str>>(
        urlencoded: S,
        plus_as_space: bool,
    ) -> Result<String, QueryParseError> {
        let mut uncoded: Vec<u8> = vec![];

        let mut chars = urlencoded.as_ref().chars().peekable();
        loop {
            match chars.next() {
                Some('+') => match plus_as_space {
                    true => uncoded.push(b' '),
                    false => uncoded.push(b'+'),
                },
                Some('%') => match chars.peek() {
                    Some(c) if c.is_ascii_hexdigit() => {
                        let upper = chars.next().unwrap();

                        if let Some(lower) = chars.peek() {
                            if lower.is_ascii_hexdigit() {
                                let upper = upper.to_digit(16).unwrap();
                                let lower = chars.next().unwrap().to_digit(16).unwrap();

                                uncoded.push(upper as u8 * 16 + lower as u8);
                                continue;
                            }
                        }

                        uncoded.push(b'%');
                        uncoded.extend_from_slice(&Self::char_bytes(upper));
                    }
                    _ => {
                        uncoded.push(b'%');
                    }
                },
                Some(c) => {
                    uncoded.extend_from_slice(&Self::char_bytes(c));
                }
                None => {
                    return Ok(String::from_utf8(uncoded).map_err(|_| QueryParseError::InvalidUtf8)?)
                }
            }
        }
    }

    /// Process a string, encoding the reserved URL characters below into their
    /// percent equivalent. Any character outside of the ASCII printables are
    /// also percent encoded.
    ///
    /// The following characters are reserved and will be encoded as %xx where
    /// x is a lowercase hex digit:
    /// `! # $ % ' ( ) * + , / : ; = ? @ [ ]`
    ///
    /// # Returns
    ///
    /// The decoded String on success or a QueryParseError if the decode resulted
    /// in invalid UTF8
    ///
    /// # Examples
    ///
    ///```rust
    ///use small_http::{Query, QueryParseError};
    ///
    ///assert_eq!(Query::url_encode("encode me spaces!"), String::from("encode%20me%20spaces%21"));
    ///assert_eq!(Query::url_encode("🥺"), String::from("%f0%9f%a5%ba"));
    ///assert_eq!(Query::url_encode("one+two"), String::from("one%2btwo"));
    ///```
    pub fn url_encode<S: AsRef<str>>(raw: S) -> String {
        let mut encoded = String::new();
        let mut chars = raw.as_ref().chars().peekable();
        let should_encode = |c: char| !c.is_ascii_graphic() || "!#$%'()*+,/:;=?@[]".contains(c);

        loop {
            match chars.next() {
                Some(c) if should_encode(c) => {
                    let bytes = Self::char_bytes(c);
                    for byte in bytes {
                        encoded.push_str(&format!("%{:02x}", byte));
                    }
                }
                Some(c) => {
                    encoded.push(c);
                }
                None => return encoded,
            }
        }
    }

    fn char_bytes(c: char) -> Vec<u8> {
        let mut utf8 = vec![0; c.len_utf8()];
        c.encode_utf8(&mut utf8);
        utf8
    }
}

impl std::str::FromStr for Query {
    type Err = QueryParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parameters: Vec<Parameter> = vec![];
        let splits = s.split('&');

        for split in splits {
            let splits: Vec<&str> = split.splitn(2, '=').collect();

            match splits.len() {
                1 => parameters.push(Parameter::Bool(splits[0].into())),
                2 => parameters.push(Parameter::Value(
                    splits[0].into(),
                    Self::url_decode(splits[1], true)?,
                )),
                _ => unreachable!(),
            }
        }

        Ok(Self { parameters })
    }
}

impl IntoIterator for Query {
    type Item = Parameter;

    type IntoIter = IntoIter<Parameter>;

    fn into_iter(self) -> Self::IntoIter {
        self.parameters.into_iter()
    }
}

#[derive(Debug)]
pub enum Parameter {
    Bool(String),
    Value(String, String),
}

#[derive(Error, Debug, PartialEq)]
pub enum QueryParseError {
    #[error("the query did not resolve to valid utf8")]
    InvalidUtf8,
}
