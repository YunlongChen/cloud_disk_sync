pub fn parse_account_path(path_str: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    // 格式: account_name:/path/to/folder
    let parts: Vec<&str> = path_str.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(format!(
            "无效的路径格式，应为 account_name:/path/to/folder，实际: {}",
            path_str
        )
        .into());
    }

    let account = parts[0].trim().to_string();
    let path = parts[1].trim().to_string();

    if account.is_empty() || path.is_empty() {
        return Err("账户名或路径不能为空".into());
    }

    Ok((account, path))
}
