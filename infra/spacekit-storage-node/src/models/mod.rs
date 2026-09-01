//! Data models and response structures for the storage node API
//!

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// User response for API (migrated from old implementation)
#[derive(Debug, Serialize, Deserialize)]
pub struct UserResponse {
    pub username: String,
    pub email: String,
    pub address: String,
    pub network: String,
}

/// Encrypted user response for API (migrated from old implementation)
#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptedUserResponse {
    pub session: String,
    pub message: String,
}

/// File upload response
#[derive(Debug, Serialize, Deserialize)]
pub struct FileUploadResponse {
    pub file_id: String,
    pub filename: String,
    pub size: u64,
    pub hash: String,
    pub upload_time: String,
}

/// File listing response
#[derive(Debug, Serialize, Deserialize)]
pub struct FileListResponse {
    pub files: Vec<FileInfo>,
    pub total_count: usize,
    pub total_size: u64,
}

/// Individual file information in listings
#[derive(Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub id: String,
    pub filename: String,
    pub size: u64,
    pub hash: String,
    pub encryption_algorithm: String,
    pub content_type: Option<String>,
    pub created_at: String,
}

/// API error responses
#[derive(Debug, Error, Serialize)]
#[serde(tag = "type")]
pub enum ApiError {
    #[error("User already exists: {username}")]
    UserExists { username: String },

    #[error("User not found: {username}")]
    UserNotFound { username: String },

    #[error("File not found: {file_id}")]
    FileNotFound { file_id: String },

    #[error("Invalid request: {message}")]
    InvalidRequest { message: String },

    #[error("Database error: {message}")]
    DatabaseError { message: String },

    #[error("Encryption error: {message}")]
    EncryptionError { message: String },

    #[error("Authentication failed: {message}")]
    AuthenticationFailed { message: String },

    #[error("Internal server error: {message}")]
    InternalError { message: String },
}
