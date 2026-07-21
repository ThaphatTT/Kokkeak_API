use async_trait::async_trait;

use crate::admin_order_service::{
    AdminCreateOrderResult, AdminOrderDeleteResult, AdminOrderDetailRow, AdminOrderListInput,
    AdminOrderPage, AdminOrderUpdateInput, AdminOrderUpdateResult,
};
use crate::traits::user::RepoError;

#[async_trait]
pub trait AdminOrderServiceRepository: Send + Sync {
    async fn create_full(
        &self,
        actor_user_guid: &str,
        idempotency_key: &str,
        correlation_id: Option<&str>,
        payload_json: &str,
    ) -> Result<AdminCreateOrderResult, RepoError>;

    async fn list(&self, input: &AdminOrderListInput) -> Result<AdminOrderPage, RepoError>;

    async fn detail(&self, order_guid: &str) -> Result<Option<AdminOrderDetailRow>, RepoError>;

    async fn update(
        &self,
        input: &AdminOrderUpdateInput,
    ) -> Result<AdminOrderUpdateResult, RepoError>;

    async fn delete(
        &self,
        order_guid: &str,
        actor_user_guid: &str,
    ) -> Result<AdminOrderDeleteResult, RepoError>;
}
