use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid release configuration: {0}")]
    Configuration(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("release security check failed: {0}")]
    Security(String),
}

pub type Result<T> = std::result::Result<T, Error>;
