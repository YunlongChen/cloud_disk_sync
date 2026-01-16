use std::env;
use std::path::PathBuf;

pub struct SystemInfo {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub config_path: PathBuf,
}

impl SystemInfo {
    pub fn new() -> Self {
        let config_path = dirs::config_dir()
            .map(|p| p.join("disksync").join("config.yaml"))
            .unwrap_or_else(|| PathBuf::from("Unknown"));

        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            os: env::consts::OS.to_string(),
            arch: env::consts::ARCH.to_string(),
            config_path,
        }
    }

    pub fn to_string(&self) -> String {
        format!(
            "Cloud Disk Sync Info\n\
             --------------------\n\
             Version: {}\n\
             OS: {} {}\n\
             Default Config Path: {}",
            self.version,
            self.os,
            self.arch,
            self.config_path.display()
        )
    }
}

pub fn print_info() {
    println!("{}", SystemInfo::new().to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_info_format() {
        let info = SystemInfo {
            version: "0.1.0".to_string(),
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            config_path: PathBuf::from("/tmp/config.yaml"),
        };

        let output = info.to_string();
        assert!(output.contains("Version: 0.1.0"));
        assert!(output.contains("OS: linux x86_64"));
        assert!(output.contains("Default Config Path: /tmp/config.yaml"));
    }
}
