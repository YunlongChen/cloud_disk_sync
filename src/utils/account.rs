use crate::config::{AccountConfig, ConfigManager};
use std::collections::HashMap;

pub fn find_account_id(config_manager: &ConfigManager, id_or_name: &str) -> Option<String> {
    find_account_id_internal(config_manager.get_accounts(), id_or_name)
}

pub fn find_account_id_internal(
    accounts: &HashMap<String, AccountConfig>,
    id_or_name: &str,
) -> Option<String> {
    if accounts.contains_key(id_or_name) {
        return Some(id_or_name.to_string());
    }
    for acc in accounts.values() {
        if acc.name == id_or_name {
            return Some(acc.id.clone());
        }
    }
    None
}
