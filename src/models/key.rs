#[derive(serde::Serialize, serde::Deserialize)]
pub struct KeyFile {
    pub version: u32,
    pub algorithm: String,
    pub key_strength: u32,
    pub salt: Vec<u8>,
    pub nonce: Vec<u8>,
    pub encrypted_key: Vec<u8>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used: Option<chrono::DateTime<chrono::Utc>>,
}
