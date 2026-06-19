//! Order repository port (พอร์ต order — M3).

use async_trait::async_trait;
use uuid::Uuid;

use crate::order::Order;
use crate::pagination::Cursor;

use super::user::RepoError;

/// Order repository contract.
#[async_trait]
pub trait OrderRepository: Send + Sync {
    /// Find by primary key.
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Order>, RepoError>;

    /// List orders for one customer (most recent first), keyset-paginated.
    async fn list_for_customer(
        &self,
        customer_id: Uuid,
        after: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<Order>, RepoError>;

    /// List orders assigned to one technician, keyset-paginated.
    async fn list_for_technician(
        &self,
        technician_id: Uuid,
        after: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<Order>, RepoError>;

    /// Persist a new order. Returns `Conflict` if the id already exists.
    async fn insert(&self, order: &Order) -> Result<(), RepoError>;
}
