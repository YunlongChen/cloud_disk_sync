use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use rand::RngCore;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

const KEY_FILE_NAME: &str = "security.key";
const ENC_PREFIX: &str = "ENC:";

pub struct SecurityManager {
    key_path: PathBuf,
    cipher: Aes256Gcm,
}

impl SecurityManager {
    pub fn new(config_dir: &Path) -> Self {
        let key_path = config_dir.join(KEY_FILE_NAME);
        let key = Self::load_or_create_key(&key_path);
        let cipher = Aes256Gcm::new(&key);

        Self { key_path, cipher }
    }

    fn load_or_create_key(path: &Path) -> Key<Aes256Gcm> {
        if path.exists() {
            match fs::read(path) {
                Ok(bytes) => {
                    if bytes.len() == 32 {
                        return *Key::<Aes256Gcm>::from_slice(&bytes);
                    }
                    warn!("Invalid key file length, regenerating key");
                }
                Err(e) => warn!("Failed to read key file: {}, regenerating", e),
            }
        }

        // Generate new key
        let mut key_bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut key_bytes);
        
        if let Err(e) = fs::write(path, &key_bytes) {
            warn!("Failed to save security key: {}", e);
        } else {
            info!("Generated new security key at {:?}", path);
        }

        *Key::<Aes256Gcm>::from_slice(&key_bytes)
    }

    pub fn encrypt(&self, plain_text: &str) -> String {
        if plain_text.starts_with(ENC_PREFIX) {
            return plain_text.to_string();
        }

        // Generate 12-byte nonce
        let mut nonce_bytes = [0u8; 12];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        match self.cipher.encrypt(nonce, plain_text.as_bytes()) {
            Ok(ciphertext) => {
                let mut combined = nonce_bytes.to_vec();
                combined.extend_from_slice(&ciphertext);
                
                // Use engine for base64
                use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
                format!("{}{}", ENC_PREFIX, BASE64.encode(combined))
            }
            Err(_) => {
                warn!("Encryption failed, returning plain text");
                plain_text.to_string()
            }
        }
    }

    pub fn decrypt(&self, text: &str) -> String {
        if !text.starts_with(ENC_PREFIX) {
            return text.to_string();
        }

        let encoded = &text[ENC_PREFIX.len()..];
        
        use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
        let bytes = match BASE64.decode(encoded) {
            Ok(b) => b,
            Err(_) => return text.to_string(), // Not valid base64, return original
        };

        if bytes.len() < 12 {
            return text.to_string();
        }

        let nonce = Nonce::from_slice(&bytes[0..12]);
        let ciphertext = &bytes[12..];

        match self.cipher.decrypt(nonce, ciphertext) {
            Ok(plaintext) => String::from_utf8(plaintext).unwrap_or_else(|_| text.to_string()),
            Err(_) => text.to_string(), // Decryption failed
        }
    }
}
