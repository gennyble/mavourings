use std::{net::TcpStream, string::FromUtf8Error};

use async_dup::Arc;
use async_io::Async;
use futures_lite::{
    io::{BufReader, BufWriter},
    AsyncBufReadExt, AsyncWriteExt,
};
use http::{method::InvalidMethod, Method, Request, Version};
use thiserror::Error;

pub use http::Response;

pub struct Connection {
    reader: BufReader<Arc<Async<TcpStream>>>,
    stream: Arc<Async<TcpStream>>,
}

impl Connection {
    pub fn new(stream: Async<TcpStream>) -> Self {
        let arcstream = Arc::new(stream);

        Self {
            reader: BufReader::new(arcstream.clone()),
            stream: arcstream,
        }
    }

    /// Attempt to parse the head of the HTTP request. The internal BufReader
    /// is left at the start of the body
    pub async fn parse_request(&mut self) -> Result<Request<()>, ConnectionError> {
        let mut buffer = Vec::new();

        loop {
            let read_count = self.reader.read_until('\n' as u8, &mut buffer).await?;

            if read_count == 0 {
                return Err(ConnectionError::NoRequest);
            }

            // Header over!
            if buffer.ends_with(b"\r\n\r\n") {
                break;
            }
        }

        let string = String::from_utf8(buffer)?;
        let mut lines = string.strip_suffix("\r\n\r\n").unwrap().lines();

        let request_line = lines.next().ok_or(ConnectionError::MissingRequest)?;
        let mut request_parts = request_line.split_ascii_whitespace();
        let method: Method = request_parts
            .next()
            .ok_or(ConnectionError::MissingRequestMethod)?
            .parse()?;
        let path = request_parts
            .next()
            .ok_or(ConnectionError::MissingRequestPath)?;
        let version = Self::version_from_str(
            request_parts
                .next()
                .ok_or(ConnectionError::MissingHttpVersion)?,
        )?;

        if request_parts.next().is_some() {
            return Err(ConnectionError::RequestTooManyWords(
                (4 + request_parts.count()) as u8,
            ));
        }

        let mut request_builder = Request::builder().method(method).uri(path).version(version);

        // headers
        for line in lines {
            let (key, value) = match line.split_once(':') {
                Some(tuple) => tuple,
                None => return Err(ConnectionError::InvalidHeader(line.to_owned())),
            };

            request_builder = request_builder.header(key, value);
        }

        //todo: verify body is content-length long

        Ok(request_builder.body(())?)
    }

    fn version_from_str<S: AsRef<str>>(string: S) -> Result<Version, ConnectionError> {
        match string.as_ref() {
            "HTTP/0.9" => Ok(Version::HTTP_09),
            "HTTP/1.0" => Ok(Version::HTTP_10),
            "HTTP/1.1" => Ok(Version::HTTP_11),
            "HTTP/2.0" => Err(ConnectionError::UnsupportedHttpVersion),
            "HTTP/3.0" => Err(ConnectionError::UnsupportedHttpVersion),
            _ => Err(ConnectionError::InvalidHttpVersion(
                string.as_ref().to_owned(),
            )),
        }
    }

    fn str_from_version(v: Version) -> &'static str {
        match v {
            Version::HTTP_09 => "HTTP/0.9",
            Version::HTTP_10 => "HTTP/1.0",
            Version::HTTP_11 => "HTTP/1.1",
            Version::HTTP_2 => "HTTP/2.0",
            Version::HTTP_3 => "HTTP/3.0",
            _ => todo!(),
        }
    }

    pub async fn respond(self, response: Response<Vec<u8>>) -> Result<(), ConnectionError> {
        let status_line = format!(
            "{} {}\r\n",
            Self::str_from_version(response.version()),
            response.status()
        );

        let mut writer = BufWriter::new(self.stream);

        writer.write_all(status_line.as_bytes()).await?;

        for (name, value) in response.headers() {
            writer.write_all(name.as_str().as_bytes()).await?;
            writer.write_all(b":").await?;
            writer.write_all(value.as_bytes()).await?;
            writer.write_all(b"\r\n").await?;
        }
        writer.write_all(b"\r\n").await?;

        writer.write(response.body()).await?;
        writer.flush().await?;

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("an error occured while trying to read from the stream")] //todo: better error message
    ReadError(#[from] std::io::Error),

    #[error("no data was sent")]
    NoRequest,
    #[error("there were {0} seperate words in the request but the max is 3")]
    RequestTooManyWords(u8),
    #[error("an unknown error occured: {0}")]
    UnknownError(#[from] http::Error),

    #[error("the header contained had invalid utf8")]
    InvalidUtf8InHeader(#[from] FromUtf8Error),
    #[error("there was no request in the read data")]
    MissingRequest,
    #[error("request is missing the method")]
    MissingRequestMethod,
    #[error("invalid method: {0}")]
    InvalidRequestMethod(#[from] InvalidMethod),
    #[error("request is missing the path")]
    MissingRequestPath,
    #[error("request is missing the http version")]
    MissingHttpVersion,
    #[error("the server only support shttp versions 1.1 and below")]
    UnsupportedHttpVersion,
    #[error("{0} is not a valid http version")]
    InvalidHttpVersion(String),

    #[error("headers should be in the foramt of key:value. '{0}' is not")]
    InvalidHeader(String),
}
