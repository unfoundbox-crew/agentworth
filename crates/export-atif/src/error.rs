use thiserror::Error;

#[derive(Debug, Error)]
pub enum AtifExportError {
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Export error: {0}")]
    Custom(String),
}

pub type Result<T> = std::result::Result<T, AtifExportError>;
