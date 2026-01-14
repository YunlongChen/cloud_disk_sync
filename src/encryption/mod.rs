pub mod types;

use crate::config::EncryptionConfig;
use crate::encryption::types::EncryptionAlgorithm;
use crate::error::EncryptionError;
use aes_gcm::aead::{Aead, Nonce};
use aes_gcm::{Aes256Gcm, KeyInit};
use rand::rngs::OsRng;
use rand::RngCore;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct EncryptionManager {
    key_store: HashMap<String, Vec<u8>>,
    rng: OsRng,
}
struct Hmac {}

struct EncryptionMetadata {
    algorithm: EncryptionAlgorithm,
    key_id: String,
    nonce: Vec<u8>,
    hmac: Hmac,
}

impl EncryptionManager {
    pub fn new() -> Self {
        Self {
            key_store: HashMap::new(),
            rng: OsRng,
        }
    }

    pub async fn encrypt_file(
        &mut self,
        path: &Path,
        config: &EncryptionConfig,
    ) -> Result<(Option<PathBuf>, Option<EncryptionMetadata>), EncryptionError> {
        let key = self.get_key(&config.key_id)?;
        let cipher = Aes256Gcm::new(&key.into());

        // 生成随机nonce
        let mut nonce_bytes = [0u8; 12];
        self.rng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // 读取文件
        let data = tokio::fs::read(path).await?;

        // 加密数据
        let ciphertext = cipher.encrypt(nonce, data.as_ref())
            .map_err(|_| EncryptionError::EncryptionFailed)?;

        // 创建临时加密文件
        let temp_path = self.create_temp_path()?;
        let mut file = tokio::fs::File::create(&temp_path).await?;

        // 写入nonce和密文
        use tokio::io::AsyncWriteExt;
        file.write_all(&nonce_bytes).await?;
        file.write_all(&ciphertext).await?;

        let metadata = EncryptionMetadata {
            algorithm: config.algorithm.clone(),
            key_id: config.key_id.clone(),
            nonce: nonce_bytes.to_vec(),
            hmac: self.calculate_hmac(&ciphertext)?,
        };

        Ok((Some(temp_path), Some(metadata)))
    }

    pub async fn decrypt_file(
        &self,
        encrypted_path: &Path,
        metadata: &EncryptionMetadata,
    ) -> Result<PathBuf, EncryptionError> {
        let key = self.get_key(&metadata.key_id)?;
        let cipher = Aes256Gcm::new(&key.into());

        // 读取加密文件
        let data = tokio::fs::read(encrypted_path).await?;

        if data.len() < 12 {
            return Err(EncryptionError::InvalidData);
        }

        let (nonce_bytes, ciphertext) = data.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        // 解密数据
        let plaintext = cipher.decrypt(nonce, ciphertext)
            .map_err(|_| EncryptionError::DecryptionFailed)?;

        // 验证HMAC
        if metadata.hmac != self.calculate_hmac(ciphertext)? {
            return Err(EncryptionError::IntegrityCheckFailed);
        }

        // 写入解密文件
        let temp_path = self.create_temp_path()?;
        tokio::fs::write(&temp_path, &plaintext).await?;

        Ok(temp_path)
    }

    fn calculate_hmac(&self, ciphertext: &[u8]) -> Result<()> {
        todo!()
    }
}