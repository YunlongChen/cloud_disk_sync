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

pub fn truncate_string(s: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthChar;

    let mut width = 0;
    let mut result = String::new();
    for c in s.chars() {
        let w = UnicodeWidthChar::width(c).unwrap_or(0);
        if width + w > max_width {
            if width + 3 <= max_width {
                result.push_str("...");
            }
            break;
        }
        width += w;
        result.push(c);
    }
    result
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
