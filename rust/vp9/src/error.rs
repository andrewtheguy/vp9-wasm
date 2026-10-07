//! What a frame can fail with.

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The bitstream is malformed, or refers to a frame the decoder does not
    /// have (a reference lost to a dropped frame).
    Invalid(String),
    /// The stream is valid VP9 but not the shape this decoder takes.
    Unsupported(String),
}

impl Error {
    #[cold]
    pub fn invalid(msg: impl Into<String>) -> Self {
        Error::Invalid(msg.into())
    }
    #[cold]
    pub fn unsupported(msg: impl Into<String>) -> Self {
        Error::Unsupported(msg.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Invalid(msg) => write!(f, "invalid stream: {msg}"),
            Error::Unsupported(msg) => write!(f, "unsupported stream: {msg}"),
        }
    }
}

impl std::error::Error for Error {}
