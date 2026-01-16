use crate::config::AccountConfig;
use crate::services::provider_factory::create_provider;

pub async fn verify_account_connection(
    account: &AccountConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let provider = create_provider(account).await?;
    let _ = provider.list("/").await?;
    Ok(())
}
