

use async_trait::async_trait;
use uuid::Uuid;

use crate::order::Order;
use crate::pagination::Cursor;

use super::user::RepoError;

#[async_trait]
pub trait OrderRepository: Send + Sync {

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Order>, RepoError>;

    async fn list_for_customer(
        &self,
        customer_id: Uuid,
        after: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<Order>, RepoError>;

    async fn list_for_technician(
        &self,
        technician_id: Uuid,
        after: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<Order>, RepoError>;

    async fn insert(&self, order: &Order) -> Result<(), RepoError>;
}
