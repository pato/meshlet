use thiserror::Error;

#[derive(Error, Debug)]
pub enum MeshletError {
    #[error("bookmark not found: {0}")]
    BookmarkNotFound(String),

    #[error("loro error: {0}")]
    LoroError(#[from] loro::LoroError),

    #[error("serialization error: {0}")]
    SerializationError(String),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, MeshletError>;