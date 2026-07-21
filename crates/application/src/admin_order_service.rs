use std::sync::Arc;

use kokkak_domain::admin_order_service::{
    AdminCreateOrderResult, AdminOrderDeleteResult, AdminOrderDetailRow, AdminOrderListInput,
    AdminOrderPage, AdminOrderUpdateInput, AdminOrderUpdateResult,
};
use kokkak_domain::traits::admin_order_service::AdminOrderServiceRepository;
use kokkak_domain::traits::user::RepoError;

struct DisabledAdminOrderServiceRepository;

#[async_trait::async_trait]
impl AdminOrderServiceRepository for DisabledAdminOrderServiceRepository {
    async fn create_full(
        &self,
        _actor_user_guid: &str,
        _idempotency_key: &str,
        _correlation_id: Option<&str>,
        _payload_json: &str,
    ) -> Result<AdminCreateOrderResult, RepoError> {
        Err(RepoError::Backend(
            "admin_order_service not configured".into(),
        ))
    }

    async fn list(&self, _input: &AdminOrderListInput) -> Result<AdminOrderPage, RepoError> {
        Err(RepoError::Backend(
            "admin_order_service not configured".into(),
        ))
    }

    async fn detail(&self, _order_guid: &str) -> Result<Option<AdminOrderDetailRow>, RepoError> {
        Err(RepoError::Backend(
            "admin_order_service not configured".into(),
        ))
    }

    async fn update(
        &self,
        _input: &AdminOrderUpdateInput,
    ) -> Result<AdminOrderUpdateResult, RepoError> {
        Err(RepoError::Backend(
            "admin_order_service not configured".into(),
        ))
    }

    async fn delete(
        &self,
        _order_guid: &str,
        _actor_user_guid: &str,
    ) -> Result<AdminOrderDeleteResult, RepoError> {
        Err(RepoError::Backend(
            "admin_order_service not configured".into(),
        ))
    }
}

pub struct AdminOrderService {
    repo: Arc<dyn AdminOrderServiceRepository>,
}

impl AdminOrderService {
    pub fn new(repo: Arc<dyn AdminOrderServiceRepository>) -> Self {
        Self { repo }
    }

    pub fn disabled() -> Self {
        Self {
            repo: Arc::new(DisabledAdminOrderServiceRepository),
        }
    }

    pub async fn create_full(
        &self,
        actor_user_guid: &str,
        idempotency_key: &str,
        correlation_id: Option<&str>,
        payload_json: &str,
    ) -> Result<AdminCreateOrderResult, RepoError> {
        self.repo
            .create_full(
                actor_user_guid,
                idempotency_key,
                correlation_id,
                payload_json,
            )
            .await
    }

    pub async fn list(&self, input: AdminOrderListInput) -> Result<AdminOrderPage, RepoError> {
        self.repo.list(&input).await
    }

    pub async fn detail(&self, order_guid: &str) -> Result<Option<AdminOrderDetailRow>, RepoError> {
        self.repo.detail(order_guid).await
    }

    pub async fn update(
        &self,
        input: AdminOrderUpdateInput,
    ) -> Result<AdminOrderUpdateResult, RepoError> {
        self.repo.update(&input).await
    }

    pub async fn delete(
        &self,
        order_guid: &str,
        actor_user_guid: &str,
    ) -> Result<AdminOrderDeleteResult, RepoError> {
        self.repo.delete(order_guid, actor_user_guid).await
    }
}
