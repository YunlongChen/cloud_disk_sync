pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let base = 1024.0;
    let exponent = (bytes as f64).log(base).floor() as u32;
    let unit = UNITS[exponent.min(5) as usize];
    let value = bytes as f64 / base.powi(exponent as i32);
    format!("{:.2} {}", value, unit)
}

#[cfg(test)]
mod tests {
    use super::format_bytes;
    #[test]
    fn test_format_bytes_basic() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
    }
}
