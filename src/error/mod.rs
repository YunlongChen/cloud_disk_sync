use reqwest::Error as ReqwestError;
use rusqlite::Error as RusqliteError;
use serde_json::Error as SerdeJsonError;
use std::io::Error as IoError;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SyncError {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    #[error("Encryption error: {0}")]
    Encryption(#[from] EncryptionError),

    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("IO error: {0}")]
    Io(#[from] IoError),

    #[error("Network error: {0}")]
    Network(#[from] ReqwestError),

    #[error("Database error: {0}")]
    Database(#[from] RusqliteError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] SerdeJsonError),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Timeout error: {0}")]
    Timeout(String),

    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),

    #[error("Conflict detected: {0}")]
    Conflict(String),

    #[error("Integrity check failed: {0}")]
    IntegrityCheckFailed(String),

    #[error("Retry limit exceeded: {0}")]
    RetryLimitExceeded(String),

    #[error("Operation canceled: ")]
    OperationCanceled,

    #[error("Unsupported feature: {0}")]
    Unsupported(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Configuration file not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Failed to read config file: {0}")]
    ReadFailed(#[source] IoError),

    #[error("Failed to parse config: {0}")]
    ParseFailed(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid configuration: {0}")]
    Invalid(String),

    #[error("Configuration directory not found")]
    NoConfigDir,
}

#[derive(Error, Debug)]
pub enum ProviderError {
    #[error("Provider not found: {0}")]
    NotFound(String),

    #[error("Provider not supported: {0}")]
    NotSupported(String),

    #[error("Missing credentials for provider: {0}")]
    MissingCredentials(String),

    #[error("Invalid credentials: {0}")]
    InvalidCredentials(String),

    #[error("Provider quota exceeded: {0}")]
    QuotaExceeded(String),

    #[error("Provider rate limited: {0}")]
    RateLimited(String),

    #[error("Provider authentication failed: {0}")]
    AuthFailed(String),

    #[error("Provider connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Provider API error: {0}")]
    ApiError(String),

    #[error("Provider timeout: {0}")]
    Timeout(String),

    #[error("Provider file not found: {0}")]
    FileNotFound(String),

    #[error("Provider permission denied: {0}")]
    PermissionDenied(String),

    #[error("Feature not implemented: {0}")]
    NotImplemented(String),
}

#[derive(Error, Debug)]
pub enum EncryptionError {
    #[error("Encryption key not found: {0}")]
    KeyNotFound(String),

    #[error("Invalid encryption key: {0}")]
    InvalidKey(String),

    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),

    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),

    #[error("Invalid initialization vector")]
    InvalidIV,

    #[error("Integrity check failed")]
    IntegrityCheckFailed,

    #[error("Invalid encrypted data")]
    InvalidData,

    #[error("Unsupported algorithm: {0}")]
    UnsupportedAlgorithm(String),
}

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Storage full: {0}")]
    Full(String),

    #[error("Storage not available: {0}")]
    NotAvailable(String),

    #[error("Storage timeout: {0}")]
    Timeout(String),

    #[error("Storage corruption detected: {0}")]
    Corruption(String),

    #[error("Storage version mismatch: {0}")]
    VersionMismatch(String),
}

// 为错误类型实现一些便利方法
impl SyncError {
    pub fn is_retryable(&self) -> bool {
        match self {
            SyncError::Network(_)
            | SyncError::Timeout(_)
            | SyncError::RateLimitExceeded(_)
            | SyncError::ResourceExhausted(_) => true,
            SyncError::Provider(ProviderError::RateLimited(_))
            | SyncError::Provider(ProviderError::Timeout(_))
            | SyncError::Provider(ProviderError::ConnectionFailed(_)) => true,
            _ => false,
        }
    }

    pub fn is_fatal(&self) -> bool {
        match self {
            SyncError::Provider(ProviderError::NotFound(_))
            | SyncError::Provider(ProviderError::NotSupported(_))
            | SyncError::Provider(ProviderError::InvalidCredentials(_))
            | SyncError::Provider(ProviderError::PermissionDenied(_))
            | SyncError::Encryption(EncryptionError::KeyNotFound(_))
            | SyncError::Encryption(EncryptionError::InvalidKey(_))
            | SyncError::Validation(_) => true,
            _ => false,
        }
    }

    pub fn error_code(&self) -> u32 {
        match self {
            SyncError::Config(_) => 1000,
            SyncError::Provider(_) => 2000,
            SyncError::Encryption(_) => 3000,
            SyncError::Storage(_) => 4000,
            SyncError::Io(_) => 5000,
            SyncError::Network(_) => 6000,
            SyncError::Database(_) => 7000,
            SyncError::Serialization(_) => 9000,
            SyncError::Validation(_) => 10000,
            SyncError::Timeout(_) => 11000,
            SyncError::RateLimitExceeded(_) => 12000,
            SyncError::AuthenticationFailed(_) => 13000,
            SyncError::PermissionDenied(_) => 14000,
            SyncError::FileNotFound(_) => 15000,
            SyncError::ResourceExhausted(_) => 16000,
            SyncError::Conflict(_) => 17000,
            SyncError::IntegrityCheckFailed(_) => 18000,
            SyncError::RetryLimitExceeded(_) => 19000,
            SyncError::OperationCanceled => 20000,
            SyncError::Unsupported(_) => 21000,
            SyncError::Unknown(_) => 99999,
        }
    }
}

// Result 类型别名
pub type Result<T> = std::result::Result<T, SyncError>;
pub type ConfigResult<T> = std::result::Result<T, ConfigError>;
pub type ProviderResult<T> = std::result::Result<T, ProviderError>;
pub type EncryptionResult<T> = std::result::Result<T, EncryptionError>;
