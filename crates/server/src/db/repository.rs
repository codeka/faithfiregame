pub trait Storage: Send + Sync {
    fn create_dev_session(
        &self,
        username: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<(u64, String)>> + Send;

    fn validate_session(
        &self,
        token: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<Option<u64>>> + Send;
}
