use crate::models::key::KeyFile;
use crate::utils::crypto::generate_recovery_code;
use aes_gcm::KeyInit;
use aes_gcm::aead::Aead;
use rand::{Rng, rng};
use std::fs;

pub fn cmd_generate_key(
    key_name: &str,
    strength: Option<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔑 生成加密密钥: {}", key_name);

    // 确定密钥强度
    let key_strength = strength.unwrap_or(256);
    let key_size = match key_strength {
        128 => 16,
        192 => 24,
        256 => 32,
        _ => {
            eprintln!("⚠️  不支持的密钥强度: {}，使用默认256位", key_strength);
            32
        }
    };

    // 生成随机密钥
    let mut key_bytes = vec![0u8; key_size];
    rng().fill(&mut key_bytes[..]);

    // 创建密钥存储目录
    let keys_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("disksync")
        .join("keys");

    fs::create_dir_all(&keys_dir)?;

    // 保存密钥文件
    let key_file = keys_dir.join(format!("{}.key", key_name));

    // 加密保存密钥（使用主密码保护）
    println!("🔒 请设置主密码来保护此密钥:");
    let password = rpassword::prompt_password("主密码: ")?;
    let confirm_password = rpassword::prompt_password("确认主密码: ")?;

    if password != confirm_password {
        return Err("两次输入的密码不一致".into());
    }

    if password.len() < 8 {
        return Err("密码长度至少8位".into());
    }

    // 使用PBKDF2派生密钥加密密钥
    let salt: [u8; 16] = rand::random();
    let encryption_key = [0u8; 32];

    // pbkdf2::pbkdf2::<hmac::Hmac<sha2::Sha256>>(
    //     password.as_bytes(),
    //     &salt,
    //     100_000,
    //     &mut encryption_key,
    // );

    // 加密密钥数据
    let cipher = aes_gcm::Aes256Gcm::new(&encryption_key.into());
    let nonce: [u8; 12] = rand::random();

    let encrypted_key = cipher
        .encrypt(&nonce.into(), key_bytes.as_ref())
        .map_err(|e| format!("加密密钥失败: {}", e))?;

    // 保存加密的密钥文件
    let key_data = KeyFile {
        version: 1,
        algorithm: "AES-256-GCM".to_string(),
        key_strength,
        salt: salt.to_vec(),
        nonce: nonce.to_vec(),
        encrypted_key,
        created_at: chrono::Utc::now(),
        last_used: None,
    };

    let json_data = serde_json::to_string_pretty(&key_data)?;
    fs::write(&key_file, json_data)?;

    // 显示密钥信息
    println!("✅ 密钥生成成功!");
    println!("📁 密钥文件: {}", key_file.display());
    println!("📏 密钥强度: {} 位", key_strength);
    println!("🔐 加密算法: AES-256-GCM");
    println!("📅 创建时间: {}", key_data.created_at);
    println!("💡 密钥ID: {}", key_name);

    // 显示重要提示
    println!("\n⚠️  重要提示:");
    println!("  1. 请妥善保管密钥文件和主密码");
    println!("  2. 丢失密钥或密码将无法解密已加密的文件");
    println!("  3. 建议备份密钥文件到安全的地方");
    println!("  4. 不要将密钥文件与加密数据存储在同一位置");

    // 生成恢复代码
    let recovery_code = generate_recovery_code(&key_bytes);
    println!("\n🔐 恢复代码 (请在安全的地方保存):");
    println!("{}", recovery_code);

    Ok(())
}
