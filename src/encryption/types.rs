// src/encryption/types.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EncryptionAlgorithm {
    /// AES-GCM with 256-bit key
    Aes256Gcm,
    /// AES-CBC with 256-bit key
    Aes256Cbc,
    /// ChaCha20-Poly1305
    ChaCha20Poly1305,
    /// XChaCha20-Poly1305
    XChaCha20Poly1305,
    /// AES-GCM-SIV
    Aes256GcmSiv,
}

impl EncryptionAlgorithm {
    pub fn key_size(&self) -> usize {
        match self {
            Self::Aes256Gcm => 32,
            Self::Aes256Cbc => 32,
            Self::ChaCha20Poly1305 => 32,
            Self::XChaCha20Poly1305 => 32,
            Self::Aes256GcmSiv => 32,
        }
    }

    pub fn iv_size(&self) -> usize {
        match self {
            Self::Aes256Gcm => 12,
            Self::Aes256Cbc => 16,
            Self::ChaCha20Poly1305 => 12,
            Self::XChaCha20Poly1305 => 24,
            Self::Aes256GcmSiv => 12,
        }
    }

    pub fn tag_size(&self) -> usize {
        match self {
            Self::Aes256Gcm => 16,
            Self::Aes256Cbc => 0, // CBC模式需要单独计算HMAC
            Self::ChaCha20Poly1305 => 16,
            Self::XChaCha20Poly1305 => 16,
            Self::Aes256GcmSiv => 16,
        }
    }

    pub fn recommended_iv_mode(&self) -> IvMode {
        match self {
            Self::Aes256Gcm => IvMode::Random,
            Self::Aes256Cbc => IvMode::Random,
            Self::ChaCha20Poly1305 => IvMode::Random,
            Self::XChaCha20Poly1305 => IvMode::Random,
            Self::Aes256GcmSiv => IvMode::Random,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum IvMode {
    /// 随机生成IV（推荐）
    Random,
    /// 从文件数据派生IV（基于内容）
    Derived,
    /// 使用固定IV（不推荐，仅用于测试）
    Fixed,
    /// 基于计数器生成IV
    Counter,
    /// 基于文件偏移量生成IV
    FileOffset,
}

impl IvMode {
    pub fn requires_unique_per_file(&self) -> bool {
        match self {
            Self::Random => true,
            Self::Derived => true,
            Self::Fixed => false,
            Self::Counter => true,
            Self::FileOffset => true,
        }
    }

    pub fn generate_iv(&self, context: &IvContext) -> Vec<u8> {
        match self {
            Self::Random => self.generate_random_iv(context.iv_size),
            Self::Derived => self.generate_derived_iv(context),
            Self::Fixed => context.fixed_iv.clone().unwrap_or_default(),
            Self::Counter => self.generate_counter_iv(context),
            Self::FileOffset => self.generate_file_offset_iv(context),
        }
    }

    fn generate_random_iv(&self, size: usize) -> Vec<u8> {
        let mut iv = vec![0u8; size];
        for b in iv.iter_mut() {
            *b = rand::random();
        }
        iv
    }

    fn generate_derived_iv(&self, context: &IvContext) -> Vec<u8> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();

        if let Some(data) = &context.data {
            hasher.update(data);
        }

        if let Some(file_path) = &context.file_path {
            hasher.update(file_path.as_bytes());
        }

        if let Some(file_hash) = &context.file_hash {
            hasher.update(file_hash);
        }

        let result = hasher.finalize();
        result[..context.iv_size].to_vec()
    }

    fn generate_counter_iv(&self, context: &IvContext) -> Vec<u8> {
        let counter = context.counter.unwrap_or(0);
        let mut iv = vec![0u8; context.iv_size];

        // 将计数器编码到IV中
        for i in 0..8 {
            if i < context.iv_size {
                iv[i] = ((counter >> (i * 8)) & 0xFF) as u8;
            }
        }

        iv
    }

    fn generate_file_offset_iv(&self, context: &IvContext) -> Vec<u8> {
        let offset = context.file_offset.unwrap_or(0);
        let mut iv = vec![0u8; context.iv_size];

        for i in 0..8 {
            if i < context.iv_size {
                iv[i] = ((offset >> (i * 8)) & 0xFF) as u8;
            }
        }

        iv
    }
}

#[derive(Debug, Clone)]
pub struct IvContext {
    pub iv_size: usize,
    pub data: Option<Vec<u8>>,
    pub file_path: Option<String>,
    pub file_hash: Option<String>,
    pub counter: Option<u64>,
    pub file_offset: Option<u64>,
    pub fixed_iv: Option<Vec<u8>>,
}
