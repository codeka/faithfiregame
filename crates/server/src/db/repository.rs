use shared::character::CharacterDefinition;
use shared::entity::InventoryConfig;

#[derive(Debug, Clone)]
pub struct CharacterSaveRecord {
    pub account_id: u64,
    pub area_id: String,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub appearance: CharacterDefinition,
    pub inventory: InventoryConfig,
}

pub trait Storage: Send + Sync {
    fn create_guest_session(
        &self,
    ) -> impl std::future::Future<Output = anyhow::Result<(u64, String)>> + Send;

    #[allow(dead_code)]
    fn register_account(
        &self,
        existing_account_id: Option<u64>,
        username: &str,
        password: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<u64>> + Send;

    fn login_with_password(
        &self,
        username: &str,
        password: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<Option<(u64, String)>>> + Send;

    fn validate_session(
        &self,
        token: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<Option<u64>>> + Send;

    fn delete_session(
        &self,
        token: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;

    fn load_character_save(
        &self,
        account_id: u64,
    ) -> impl std::future::Future<Output = anyhow::Result<Option<CharacterSaveRecord>>> + Send;

    fn save_character_state(
        &self,
        save: &CharacterSaveRecord,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}
