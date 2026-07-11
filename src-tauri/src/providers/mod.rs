pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod credential_store;

use crate::models::ProviderSnapshot;

pub trait UsageProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn has_local_credentials(&self) -> bool;
    fn refresh(&self) -> Result<ProviderSnapshot, String>;
}
