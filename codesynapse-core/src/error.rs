use thiserror::Error;

#[derive(Error, Debug)]
pub enum CodeSynapseError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Binary serialization error: {0}")]
    Bincode(#[from] bincode::Error),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}

impl CodeSynapseError {
    pub fn msg(msg: impl Into<String>) -> Self {
        CodeSynapseError::Other(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, CodeSynapseError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_error_display() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = CodeSynapseError::Io(io_err);
        let msg = format!("{}", err);
        assert!(msg.contains("file not found"), "msg: {msg}");
    }

    #[test]
    fn test_io_from_impl() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission");
        let err: CodeSynapseError = io_err.into();
        assert!(format!("{}", err).contains("permission"));
    }

    #[test]
    fn test_validation_error() {
        let err = CodeSynapseError::Validation("invalid input".into());
        assert_eq!(format!("{}", err), "Validation error: invalid input");
    }

    #[test]
    fn test_not_found_error() {
        let err = CodeSynapseError::NotFound("node missing".into());
        assert_eq!(format!("{}", err), "Not found: node missing");
    }

    #[test]
    fn test_database_error() {
        let err = CodeSynapseError::Database("connection failed".into());
        assert_eq!(format!("{}", err), "Database error: connection failed");
    }

    #[test]
    fn test_parse_error() {
        let err = CodeSynapseError::Parse("bad syntax".into());
        assert_eq!(format!("{}", err), "Parse error: bad syntax");
    }

    #[test]
    fn test_other_error() {
        let err = CodeSynapseError::Other("something went wrong".into());
        assert_eq!(format!("{}", err), "something went wrong");
    }

    #[test]
    fn test_serde_error() {
        let serde_err = serde_json::from_str::<()>("invalid").unwrap_err();
        let err = CodeSynapseError::Serialization(serde_err);
        let msg = format!("{}", err);
        assert!(msg.contains("Serialization error"));
    }

    #[test]
    fn test_result_type_alias() {
        let ok: Result<i32> = Ok(42);
        assert!(matches!(ok, Ok(42)));

        let err: Result<i32> = Err(CodeSynapseError::Other("fail".into()));
        assert!(err.is_err());
    }
}
