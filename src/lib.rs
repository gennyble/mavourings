mod query;

use std::{
    fs::File,
    io::Read,
    net::TcpStream,
    path::{Path, PathBuf},
    string::FromUtf8Error,
};

use async_dup::Arc;
use async_io::Async;
use futures_lite::{
    io::{BufReader, BufWriter},
    AsyncBufReadExt, AsyncReadExt, AsyncWriteExt,
};
use http::{method::InvalidMethod, request, Method, Version};
use mime_guess::{Mime, MimeGuess};
use thiserror::Error;

pub use query::{Parameter, Query, QueryParseError};

pub use http::{Request, Response, StatusCode};

enum BodyType {
    None,
    Chunked,
    ContentLength(usize),
}

#[derive(Debug, PartialEq)]
enum ConnectionKind {
    Closed,
    Close,
    KeepAlive { timeout: usize, max_requests: usize },
}

impl ConnectionKind {
    fn keepalive(timeout: usize, max_requests: usize) -> Self {
        Self::KeepAlive {
            timeout,
            max_requests,
        }
    }
}

pub struct Connection {
    reader: BufReader<Arc<Async<TcpStream>>>,
    stream: Arc<Async<TcpStream>>,
    kind: ConnectionKind,
}

impl Connection {
    pub fn new(stream: Async<TcpStream>) -> Self {
        let arcstream = Arc::new(stream);

        Self {
            reader: BufReader::new(arcstream.clone()),
            stream: arcstream,
            kind: ConnectionKind::Close,
        }
    }

    pub async fn request(&mut self) -> Result<Option<Request<Vec<u8>>>, ConnectionError> {
        if self.kind == ConnectionKind::Closed {
            return Ok(None);
        }

        let (builder, body_type) = self.parse_request().await?;

        let req = match body_type {
            BodyType::None => builder.body(vec![])?,
            BodyType::ContentLength(bytes) => {
                let mut buffer: Vec<u8> = vec![0; bytes];

                self.reader.read_exact(&mut buffer).await?;
                builder.body(buffer)?
            }
            BodyType::Chunked => builder.body(self.parse_chunked().await?)?,
        };

        // HTTP/1.0 default is close, 1.1 is keep-alive
        self.kind = match req.headers().get(http::header::CONNECTION) {
            Some(value) => match value.to_str() {
                Ok(string) if string == "close" => ConnectionKind::Close,
                Ok(string) if string.to_lowercase() == "keep-alive" => {
                    ConnectionKind::keepalive(0, 0)
                }
                _ => ConnectionKind::Close,
            },
            None => ConnectionKind::Close,
        };

        Ok(Some(req))
    }

    async fn parse_chunked(&mut self) -> Result<Vec<u8>, ConnectionError> {
        let mut buffer: Vec<u8> = vec![];

        loop {
            let mut length_string = String::new();
            self.reader.read_line(&mut length_string).await?;

            let length_str = match length_string.strip_suffix("\r\n") {
                Some(len) => len,
                None => return Err(ConnectionError::ChunkedLengthMissingCRLF),
            };

            let length = usize::from_str_radix(length_str, 10)
                .map_err(|_| ConnectionError::InvalidChunkedLength(length_string))?;

            if length == 0 {
                let mut onlycrlf = String::with_capacity(2);
                self.reader.read_line(&mut onlycrlf).await?;

                if onlycrlf == "\r\n" {
                    return Ok(buffer);
                } else {
                    return Err(ConnectionError::ChunkedDataMissingCRLF);
                }
            }

            let bytes_read = self.reader.read_until(b'\n', &mut buffer).await?;
            match (buffer.pop(), buffer.pop()) {
                (Some(a), Some(b)) if a == b'\n' && b == b'\r' => (),
                _ => return Err(ConnectionError::ChunkedDataMissingCRLF),
            }

            if length + 2 != bytes_read {
                return Err(ConnectionError::InvalidDataPart {
                    expected: length + 2,
                    got: bytes_read,
                });
            }
        }
    }

    async fn parse_request(&mut self) -> Result<(request::Builder, BodyType), ConnectionError> {
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
        let mut body_type = BodyType::None;

        // headers
        for line in lines {
            let (key, value) = match line.split_once(':') {
                Some(tuple) => tuple,
                None => return Err(ConnectionError::InvalidHeader(line.to_owned())),
            };

            if key.to_lowercase() == "transfer-encoding" && value.to_lowercase().contains("chunked")
            {
                body_type = BodyType::Chunked;
            } else if key.to_lowercase() == "content-length" {
                body_type = BodyType::ContentLength(
                    usize::from_str_radix(value.trim(), 10)
                        .map_err(|_| ConnectionError::InvalidContentLength(value.into()))?,
                );
            }

            request_builder = request_builder.header(key, value.trim());
        }

        //todo: verify body is content-length long

        Ok((request_builder, body_type))
    }

    fn version_from_str<S: AsRef<str>>(string: S) -> Result<Version, ConnectionError> {
        match string.as_ref() {
            "HTTP/0.9" => Err(ConnectionError::UnsupportedHttpVersion),
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

    pub async fn respond(&mut self, response: Response<Vec<u8>>) -> Result<(), ConnectionError> {
        let status_line = format!(
            "{} {}\r\n",
            Self::str_from_version(response.version()),
            response.status()
        );

        let mut writer = BufWriter::new(self.stream.clone());

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

        match self.kind {
            ConnectionKind::Close => {
                self.stream.close().await?;
                self.kind = ConnectionKind::Closed;
            }
            _ => (),
        }

        Ok(())
    }

    pub async fn send_file_unchecked<P: AsRef<Path>>(
        &mut self,
        path: P,
        forced_mime: Option<Mime>,
    ) -> Result<(), ConnectionError> {
        let path = path.as_ref();
        let mime = if let Some(mime) = forced_mime {
            mime
        } else {
            MimeGuess::from_path(path).first_or_octet_stream()
        };

        let mut buffer = vec![];
        let mut file = File::open(path)?;
        file.read_to_end(&mut buffer)?;

        self.respond(
            Response::builder()
                .header("content-type", mime.to_string())
                .header("content-length", buffer.len())
                .body(buffer)?,
        )
        .await
    }

    pub async fn send_file<R: Into<PathBuf>, S: AsRef<Path>>(
        &mut self,
        root: R,
        file: S,
        forced_mime: Option<Mime>,
    ) -> Result<(), ConnectionError> {
        let root: PathBuf = root.into();
        let file = file.as_ref();

        let canonical = file.canonicalize()?;

        if canonical.starts_with(root) && canonical.is_file() {
            self.send_file_unchecked(file, forced_mime).await
        } else {
            Err(ConnectionError::FileOutsideRoot)
        }
    }
}

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("io error: {0}")] //todo: better error message
    IoError(#[from] std::io::Error),

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
    #[error("invalid content length {0}")]
    InvalidContentLength(String),

    #[error("invalid length in chunked encoding")]
    InvalidChunkedLength(String),
    #[error("chunked length missing CR LF")] //todo: better error message
    ChunkedLengthMissingCRLF,
    #[error("chunked data missing CR LF")] //todo: better error message
    ChunkedDataMissingCRLF,
    #[error("invalid data part in chunked encoding. Expect {expected} bytes. Got {got}")]
    InvalidDataPart { expected: usize, got: usize },

    #[error("could not serve the file as it resolved outside the root path")]
    FileOutsideRoot,
}
