use crate::config::AccountConfig;
use crate::utils::account::find_account_id_internal;
use crate::utils::path::parse_account_path;
use std::collections::HashMap;

pub async fn parse_account_path_or_select(
    input: &str,
    accounts: &HashMap<String, AccountConfig>,
    account_list: &[(String, String)],
    account_display: &[String],
    label: &str,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    // 尝试解析输入
    if let Ok((acc, path)) = parse_account_path(input) {
        // 检查账户是否存在
        let acc_id = find_account_id_internal(accounts, &acc);
        if let Some(id) = acc_id {
            return Ok((id, path));
        } else {
            // 账户不存在，可能是只提供了账户名，没有路径
            // 或者格式错误
        }
    }

    // 尝试作为账户ID或名称查找
    let acc_id = find_account_id_internal(accounts, input);
    if let Some(id) = acc_id {
        // 找到了账户，请求路径
        let path = dialoguer::Input::<String>::new()
            .with_prompt(format!("请输入{}路径", label))
            .default("/".to_string())
            .interact_text()?;
        return Ok((id, path));
    }

    // 无法解析，进入交互选择
    println!("⚠️  无法解析账户: {}", input);
    select_account_and_path(accounts, account_list, account_display, label).await
}

pub async fn select_account_and_path(
    accounts: &HashMap<String, AccountConfig>,
    account_list: &[(String, String)],
    account_display: &[String],
    label: &str,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    use dialoguer::{Input, Select};

    let selection = Select::new()
        .with_prompt(format!("选择{}账户", label))
        .items(account_display)
        .default(0)
        .interact()?;

    let (account_id, _) = &account_list[selection];
    let _account = accounts.get(account_id).unwrap();

    // 尝试列出目录供选择（如果支持）
    let path = Input::<String>::new()
        .with_prompt(format!("请输入{}路径", label))
        .default("/".to_string())
        .interact_text()?;

    Ok((account_id.clone(), path))
}
