pub mod types;

use crate::config::EncryptionConfig;
use crate::encryption::types::EncryptionAlgorithm;
use crate::error::EncryptionError;
use aes_gcm::aead::{Aead, Nonce};
use aes_gcm::{Aes256Gcm, KeyInit};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct EncryptionManager {
    key_store: HashMap<String, Vec<u8>>,
}
type Hmac = Vec<u8>;

pub struct EncryptionMetadata {
    pub algorithm: EncryptionAlgorithm,
    pub key_id: String,
    pub nonce: Vec<u8>,
    pub hmac: Hmac,
}

impl EncryptionManager {
    pub fn new() -> Self {
        Self {
            key_store: HashMap::new(),
        }
    }

    fn get_key(&self, key_id: &str) -> Result<Vec<u8>, EncryptionError> {
        self.key_store.get(key_id)
            .cloned()
            .ok_or_else(|| EncryptionError::KeyNotFound(key_id.to_string()))
    }

    fn create_temp_path(&self) -> Result<PathBuf, EncryptionError> {
        let name = format!("enc_{}.tmp", uuid::Uuid::new_v4());
        let path = std::env::temp_dir().join(name);
        Ok(path)
    }

    pub async fn encrypt_file(
        &self,
        path: &Path,
        config: &EncryptionConfig,
    ) -> Result<(Option<PathBuf>, Option<EncryptionMetadata>), EncryptionError> {
        let key = self.get_key(&config.key_id)?;
        let cipher = Aes256Gcm::new(aes_gcm::Key::<Aes256Gcm>::from_slice(&key));

        // 生成随机nonce
        let mut nonce_bytes = [0u8; 12];
        for b in nonce_bytes.iter_mut() {
            *b = rand::random();
        }
        let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);

        // 读取文件
        let data = tokio::fs::read(path).await
            .map_err(|e| EncryptionError::InvalidData)?;

        // 加密数据
        let ciphertext = cipher.encrypt(nonce, data.as_ref())
            .map_err(|e| EncryptionError::EncryptionFailed(e.to_string()))?;

        // 创建临时加密文件
        let temp_path = self.create_temp_path()?;
        let mut file = tokio::fs::File::create(&temp_path).await
            .map_err(|_| EncryptionError::InvalidData)?;

        // 写入nonce和密文
        use tokio::io::AsyncWriteExt;
        file.write_all(&nonce_bytes).await
            .map_err(|_| EncryptionError::InvalidData)?;
        file.write_all(&ciphertext).await
            .map_err(|_| EncryptionError::InvalidData)?;

        let metadata = EncryptionMetadata {
            algorithm: config.algorithm.clone(),
            key_id: config.key_id.clone(),
            nonce: nonce_bytes.to_vec(),
            hmac: self.calculate_hmac(&ciphertext),
        };

        Ok((Some(temp_path), Some(metadata)))
    }

    pub async fn decrypt_file(
        &self,
        encrypted_path: &Path,
        metadata: &EncryptionMetadata,
    ) -> Result<PathBuf, EncryptionError> {
        let key = self.get_key(&metadata.key_id)?;
        let cipher = Aes256Gcm::new(aes_gcm::Key::<Aes256Gcm>::from_slice(&key));

        // 读取加密文件
        let data = tokio::fs::read(encrypted_path).await
            .map_err(|_| EncryptionError::InvalidData)?;

        if data.len() < 12 {
            return Err(EncryptionError::InvalidData);
        }

        let (nonce_bytes, ciphertext) = data.split_at(12);
        let nonce = aes_gcm::Nonce::from_slice(nonce_bytes);

        // 解密数据
        let plaintext = cipher.decrypt(nonce, ciphertext)
            .map_err(|e| EncryptionError::DecryptionFailed(e.to_string()))?;

        // 验证HMAC
        if metadata.hmac != self.calculate_hmac(ciphertext) {
            return Err(EncryptionError::IntegrityCheckFailed);
        }

        // 写入解密文件
        let temp_path = self.create_temp_path()?;
        tokio::fs::write(&temp_path, &plaintext).await
            .map_err(|_| EncryptionError::InvalidData)?;

        Ok(temp_path)
    }

    fn calculate_hmac(&self, ciphertext: &[u8]) -> Hmac {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(ciphertext);
        hasher.finalize().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EncryptionConfig;
    use crate::encryption::types::{EncryptionAlgorithm, IvMode};

    #[tokio::test]
    async fn test_hmac_and_encrypt_decrypt() {
        let mut mgr = EncryptionManager::new();
        mgr.key_store.insert("test".to_string(), vec![0u8; 32]);
        let tmp = std::env::temp_dir().join("enc_test.bin");
        tokio::fs::write(&tmp, b"hello").await.unwrap();
        let cfg = EncryptionConfig {
            algorithm: EncryptionAlgorithm::Aes256Gcm,
            key_id: "test".to_string(),
            iv_mode: IvMode::Random,
        };
        let (enc_path_opt, metadata_opt) = mgr.encrypt_file(&tmp, &cfg).await.unwrap();
        assert!(enc_path_opt.is_some());
        assert!(metadata_opt.is_some());
        let enc_path = enc_path_opt.unwrap();
        let metadata = metadata_opt.unwrap();
        let dec_path = mgr.decrypt_file(&enc_path, &metadata).await.unwrap();
        let dec = tokio::fs::read(&dec_path).await.unwrap();
        assert_eq!(dec, b"hello");
    }
}
