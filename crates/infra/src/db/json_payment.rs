//! JSON-file-backed `PaymentRepository` (M9).
//!
//! Three aggregates in a single file: payments, commissions,
//! payouts. The JSON-DB sim is fine for the dev / e2e flow
//! because the data volume is tiny. Production swaps this for
//! a SQL Server-backed adapter (KOKKAK_PAYMENT database —
//! AGENTS.md § 7.1).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use kokkak_domain::{
    Commission, Payment, PaymentRepoError, PaymentRepository, PaymentStatus, Payout, PayoutStatus,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct PaymentDb {
    payments: Vec<Payment>,
    commissions: Vec<Commission>,
    payouts: Vec<Payout>,
}

struct Inner {
    db: PaymentDb,
    path: PathBuf,
    payment_by_id: std::collections::HashMap<Uuid, usize>,
    payment_by_order: std::collections::HashMap<Uuid, usize>,
    commission_by_order: std::collections::HashMap<Uuid, usize>,
}

#[derive(Clone)]
pub struct JsonPaymentRepository {
    inner: Arc<Mutex<Inner>>,
}

impl JsonPaymentRepository {
    /// Open (or create) the payment DB at `path`.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, PaymentRepoError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| PaymentRepoError::Backend(e.to_string()))?;
        }
        let db = if path.exists() {
            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|e| PaymentRepoError::Backend(e.to_string()))?;
            if bytes.is_empty() {
                PaymentDb::default()
            } else {
                serde_json::from_slice(&bytes).unwrap_or_default()
            }
        } else {
            PaymentDb::default()
        };
        let mut payment_by_id = std::collections::HashMap::new();
        let mut payment_by_order = std::collections::HashMap::new();
        for (i, p) in db.payments.iter().enumerate() {
            payment_by_id.insert(p.id, i);
            payment_by_order.insert(p.order_id, i);
        }
        let mut commission_by_order = std::collections::HashMap::new();
        for (i, c) in db.commissions.iter().enumerate() {
            commission_by_order.insert(c.order_id, i);
        }
        let inner = Inner {
            db,
            path,
            payment_by_id,
            payment_by_order,
            commission_by_order,
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    /// Build a fresh in-memory payment DB (no file IO).
    pub fn open_in_memory() -> Result<Self, PaymentRepoError> {
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                db: PaymentDb::default(),
                path: std::env::temp_dir()
                    .join(format!("kokkak_payment_inmem-{}.json", Uuid::new_v4())),
                payment_by_id: std::collections::HashMap::new(),
                payment_by_order: std::collections::HashMap::new(),
                commission_by_order: std::collections::HashMap::new(),
            })),
        })
    }

    async fn persist(inner: &Inner) -> Result<(), PaymentRepoError> {
        // In-memory variant: dummy path; skip the disk write.
        if inner.path.to_string_lossy().contains("inmem-") {
            return Ok(());
        }
        let tmp = inner.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&inner.db)
            .map_err(|e| PaymentRepoError::Backend(e.to_string()))?;
        tokio::fs::write(&tmp, &bytes)
            .await
            .map_err(|e| PaymentRepoError::Backend(e.to_string()))?;
        tokio::fs::rename(&tmp, &inner.path)
            .await
            .map_err(|e| PaymentRepoError::Backend(e.to_string()))?;
        Ok(())
    }

    fn rebuild_indexes(inner: &mut Inner) {
        inner.payment_by_id.clear();
        inner.payment_by_order.clear();
        inner.commission_by_order.clear();
        for (i, p) in inner.db.payments.iter().enumerate() {
            inner.payment_by_id.insert(p.id, i);
            inner.payment_by_order.insert(p.order_id, i);
        }
        for (i, c) in inner.db.commissions.iter().enumerate() {
            inner.commission_by_order.insert(c.order_id, i);
        }
    }
}

#[async_trait]
impl PaymentRepository for JsonPaymentRepository {
    async fn insert_payment(&self, payment: &Payment) -> Result<(), PaymentRepoError> {
        let mut g = self.inner.lock().await;
        if g.payment_by_id.contains_key(&payment.id) {
            return Err(PaymentRepoError::Backend(format!(
                "payment {} exists",
                payment.id
            )));
        }
        if g.payment_by_order.contains_key(&payment.order_id) {
            return Err(PaymentRepoError::Backend(format!(
                "order {} already has a payment",
                payment.order_id
            )));
        }
        g.db.payments.push(payment.clone());
        let i = g.db.payments.len() - 1;
        g.payment_by_id.insert(payment.id, i);
        g.payment_by_order.insert(payment.order_id, i);
        Self::persist(&g).await
    }

    async fn find_payment(&self, id: Uuid) -> Result<Option<Payment>, PaymentRepoError> {
        let g = self.inner.lock().await;
        Ok(g.payment_by_id
            .get(&id)
            .and_then(|&i| g.db.payments.get(i))
            .cloned())
    }

    async fn find_payment_by_order(
        &self,
        order_id: Uuid,
    ) -> Result<Option<Payment>, PaymentRepoError> {
        let g = self.inner.lock().await;
        Ok(g.payment_by_order
            .get(&order_id)
            .and_then(|&i| g.db.payments.get(i))
            .cloned())
    }

    async fn list_payments_for_customer(
        &self,
        customer_id: Uuid,
        limit: u32,
    ) -> Result<Vec<Payment>, PaymentRepoError> {
        let g = self.inner.lock().await;
        let limit = limit.clamp(1, 200);
        let mut out: Vec<Payment> =
            g.db.payments
                .iter()
                .filter(|p| p.customer_id == customer_id)
                .cloned()
                .collect();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        out.truncate(limit as usize);
        Ok(out)
    }

    async fn update_payment_status(
        &self,
        id: Uuid,
        status: PaymentStatus,
        gateway_ref: Option<&str>,
    ) -> Result<(), PaymentRepoError> {
        let mut g = self.inner.lock().await;
        let Some(&i) = g.payment_by_id.get(&id) else {
            return Err(PaymentRepoError::NotFound(format!("payment {id}")));
        };
        let now = chrono::Utc::now();
        g.db.payments[i].status = status;
        g.db.payments[i].updated_at = now;
        if let Some(gw) = gateway_ref {
            g.db.payments[i].gateway_ref = gw.to_string();
        }
        Self::persist(&g).await
    }

    async fn insert_commission(&self, commission: &Commission) -> Result<(), PaymentRepoError> {
        let mut g = self.inner.lock().await;
        if let Some(&i) = g.commission_by_order.get(&commission.order_id) {
            g.db.commissions[i] = commission.clone();
        } else {
            g.db.commissions.push(commission.clone());
            let i = g.db.commissions.len() - 1;
            g.commission_by_order.insert(commission.order_id, i);
        }
        Self::persist(&g).await
    }

    async fn find_commission_by_order(
        &self,
        order_id: Uuid,
    ) -> Result<Option<Commission>, PaymentRepoError> {
        let g = self.inner.lock().await;
        Ok(g.commission_by_order
            .get(&order_id)
            .and_then(|&i| g.db.commissions.get(i))
            .cloned())
    }

    async fn insert_payout(&self, payout: &Payout) -> Result<(), PaymentRepoError> {
        let mut g = self.inner.lock().await;
        g.db.payouts.push(payout.clone());
        Self::persist(&g).await
    }

    async fn list_payouts(
        &self,
        technician_id: Option<Uuid>,
        status: Option<PayoutStatus>,
        limit: u32,
    ) -> Result<Vec<Payout>, PaymentRepoError> {
        let g = self.inner.lock().await;
        let limit = limit.clamp(1, 500);
        let mut out: Vec<Payout> =
            g.db.payouts
                .iter()
                .filter(|p| match technician_id {
                    Some(t) => p.technician_id == t,
                    None => true,
                })
                .filter(|p| match status {
                    Some(s) => p.status == s,
                    None => true,
                })
                .cloned()
                .collect();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        out.truncate(limit as usize);
        Ok(out)
    }

    async fn update_payout_status(
        &self,
        id: Uuid,
        status: PayoutStatus,
    ) -> Result<(), PaymentRepoError> {
        let mut g = self.inner.lock().await;
        let now = chrono::Utc::now();
        let mut found = false;
        for p in g.db.payouts.iter_mut() {
            if p.id == id {
                p.status = status;
                p.updated_at = now;
                found = true;
            }
        }
        if !found {
            return Err(PaymentRepoError::NotFound(format!("payout {id}")));
        }
        // Rebuild indexes just in case ids changed (they don't
        // here, but the function is idempotent).
        Self::rebuild_indexes(&mut g);
        Self::persist(&g).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join("kokkak_payment_repo_test")
            .join(name)
    }

    fn sample_payment() -> Payment {
        let now = Utc::now();
        Payment {
            id: Uuid::new_v4(),
            order_id: Uuid::new_v4(),
            customer_id: Uuid::new_v4(),
            amount: Decimal::from_str("100.00").unwrap(),
            gateway_ref: "".into(),
            status: PaymentStatus::Pending,
            currency: "LAK".into(),
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn insert_and_find_by_id() {
        let path = tmp("p1.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonPaymentRepository::open(&path).await.unwrap();
        let p = sample_payment();
        let id = p.id;
        repo.insert_payment(&p).await.unwrap();
        let got = repo.find_payment(id).await.unwrap().unwrap();
        assert_eq!(got.order_id, p.order_id);
    }

    #[tokio::test]
    async fn find_payment_by_order_index() {
        let path = tmp("p2.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonPaymentRepository::open(&path).await.unwrap();
        let p = sample_payment();
        let order = p.order_id;
        repo.insert_payment(&p).await.unwrap();
        let got = repo.find_payment_by_order(order).await.unwrap();
        assert_eq!(got.unwrap().id, p.id);
    }

    #[tokio::test]
    async fn update_payment_status_persists() {
        let path = tmp("p3.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonPaymentRepository::open(&path).await.unwrap();
        let p = sample_payment();
        let id = p.id;
        repo.insert_payment(&p).await.unwrap();
        repo.update_payment_status(id, PaymentStatus::Captured, Some("pi_x"))
            .await
            .unwrap();
        let got = repo.find_payment(id).await.unwrap().unwrap();
        assert_eq!(got.status, PaymentStatus::Captured);
        assert_eq!(got.gateway_ref, "pi_x");
    }

    #[tokio::test]
    async fn commission_idempotent_on_order() {
        let path = tmp("p4.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonPaymentRepository::open(&path).await.unwrap();
        let order = Uuid::new_v4();
        let c1 = Commission {
            id: Uuid::new_v4(),
            order_id: order,
            technician_id: Uuid::new_v4(),
            gross: Decimal::from_str("100.00").unwrap(),
            amount: Decimal::from_str("50.00").unwrap(),
            rate: Decimal::from_str("0.5").unwrap(),
            net_to_tech: Decimal::from_str("50.00").unwrap(),
            computed_at: Utc::now(),
        };
        repo.insert_commission(&c1).await.unwrap();
        let c2 = Commission {
            amount: Decimal::from_str("40.00").unwrap(),
            net_to_tech: Decimal::from_str("60.00").unwrap(),
            ..c1.clone()
        };
        repo.insert_commission(&c2).await.unwrap();
        let got = repo.find_commission_by_order(order).await.unwrap().unwrap();
        assert_eq!(got.amount, Decimal::from_str("40.00").unwrap());
    }

    #[tokio::test]
    async fn list_payouts_filters_by_status() {
        let path = tmp("p5.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonPaymentRepository::open(&path).await.unwrap();
        let tech = Uuid::new_v4();
        for s in [PayoutStatus::Pending, PayoutStatus::Paid] {
            let po = Payout {
                id: Uuid::new_v4(),
                technician_id: tech,
                order_id: Uuid::new_v4(),
                amount: Decimal::from_str("50.00").unwrap(),
                status: s,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            repo.insert_payout(&po).await.unwrap();
        }
        let pending = repo
            .list_payouts(Some(tech), Some(PayoutStatus::Pending), 100)
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].status, PayoutStatus::Pending);
    }
}
