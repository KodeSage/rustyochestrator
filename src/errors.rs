use thiserror::Error;

#[derive(Error, Debug)]
pub enum RustyError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parse error: {0}")]
    YamlParse(#[from] serde_yaml::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Circular dependency detected: {0}")]
    CircularDependency(String),

    #[error("Missing dependency: task '{task}' depends on '{dep}' which does not exist")]
    MissingDependency { task: String, dep: String },
}

pub type Result<T> = std::result::Result<T, RustyError>;
