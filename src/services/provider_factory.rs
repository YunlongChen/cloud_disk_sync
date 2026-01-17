use std::error::Error;

use crate::config::{AccountConfig, ProviderType};
use crate::providers::{AliYunDriveProvider, OneOneFiveProvider, StorageProvider, WebDavProvider};

pub async fn create_provider(
    account: &AccountConfig,
) -> Result<Box<dyn StorageProvider>, Box<dyn Error>> {
    match account.provider {
        ProviderType::AliYunDrive => {
            let provider: AliYunDriveProvider = AliYunDriveProvider::new(account).await?;
            Ok(Box::new(provider))
        }
        ProviderType::WebDAV => {
            let provider: WebDavProvider = WebDavProvider::new(account).await?;
            Ok(Box::new(provider))
        }
        ProviderType::OneOneFive => {
            let provider: OneOneFiveProvider = OneOneFiveProvider::new(account).await?;
            Ok(Box::new(provider))
        }
        _ => Err(format!("Unsupported provider type: {:?}", account.provider).into()),
    }
}
