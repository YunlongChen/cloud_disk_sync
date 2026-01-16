use cloud_disk_sync::config::ConfigManager;
use std::fs;
use uuid::Uuid;

#[test]
fn test_config_reset_to_0_1_0() {
    let dir = std::env::temp_dir().join(format!("test_config_{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.yaml");

    // Create a "fake" old config with version 1.1.0 and plaintext credentials
    let old_config_content = r#"
version: "1.1.0"
global_settings:
  log_level: "Info"
  log_retention_days: 30
  max_concurrent_tasks: 5
  default_retry_policy:
    max_retries: 3
    initial_delay_ms: 1000
    max_delay_ms: 10000
    backoff_factor: 2.0
  enable_telemetry: false
  auto_update_check: true
  ui_language: "en"
accounts:
  - id: "acc1"
    provider: "WebDAV"
    name: "test"
    credentials:
      username: "user"
      password: "plaintext_password"
      url: "http://localhost"
    retry_policy:
      max_retries: 3
      initial_delay_ms: 1000
      max_delay_ms: 10000
      backoff_factor: 2.0
tasks: []
encryption_keys: []
plugins: []
schedules: []
"#;
    fs::write(&config_path, old_config_content).unwrap();

    // Initialize ConfigManager
    // This should trigger the "migration" (reset) to 0.1.0 and encryption
    let manager = ConfigManager::new_with_path(config_path.clone()).unwrap();

    // Save to persist changes
    manager.save().unwrap();

    // Read file back to verify version and encryption
    let content = fs::read_to_string(&config_path).unwrap();
    println!("Config content: {}", content);

    assert!(
        content.contains("version: 0.1.0") || content.contains(r#"version: "0.1.0""#),
        "Version should be reset to 0.1.0"
    );
    assert!(
        !content.contains("plaintext_password"),
        "Password should be encrypted"
    );
    assert!(content.contains("ENC:"), "Should contain encrypted prefix");

    // Verify we can load it back and get the correct password
    let manager2 = ConfigManager::new_with_path(config_path).unwrap();
    let account = manager2.get_account("acc1").unwrap();
    assert_eq!(
        account.credentials.get("password").map(|s| s.as_str()),
        Some("plaintext_password")
    );
}
