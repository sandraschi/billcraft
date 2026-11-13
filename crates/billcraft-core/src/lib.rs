#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

pub use billcraft_store::*;
use chrono::Local;
use craft_biz::bus::Bus;
use craft_biz::BizError;
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BillCoreError {
    #[error("Store error: {0}")]
    Store(#[from] BillStoreError),
    #[error("Bus error: {0}")]
    Bus(#[from] BizError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Validation error: {0}")]
    Validation(String),
}

pub struct BillService {
    store: BillStore,
    bus: Option<Bus>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DrainSummary {
    pub processed: usize,
    pub created_invoices: Vec<String>,
}

impl BillService {
    pub fn new(store: BillStore, bus: Option<Bus>) -> Self {
        Self { store, bus }
    }

    pub fn store(&self) -> &BillStore {
        &self.store
    }

    pub fn bus(&self) -> Option<&Bus> {
        self.bus.as_ref()
    }

    pub fn issue_invoice(&self, id: &str) -> Result<Invoice, BillCoreError> {
        let inv = self.store.get_invoice(id)?;
        if inv.gross_cents == 0 {
            return Err(BillCoreError::Validation(
                "Cannot issue empty invoice with 0.00 total".into(),
            ));
        }
        self.store.update_status(id, InvoiceStatus::Issued)?;
        let updated = self.store.get_invoice(id)?;

        if let Some(bus) = &self.bus {
            let payload = json!({
                "invoice_id": updated.id,
                "customer_id": updated.customer_id,
                "customer_name": updated.customer_name,
                "gross_cents": updated.gross_cents,
                "due_date": updated.due_date.to_string(),
            });
            let _ = bus.emit("billcraft", "gaincraft", "invoice.issued", payload);
        }

        Ok(updated)
    }

    pub fn mark_paid(&self, id: &str) -> Result<Invoice, BillCoreError> {
        self.store.update_status(id, InvoiceStatus::Paid)?;
        let updated = self.store.get_invoice(id)?;

        if let Some(bus) = &self.bus {
            let payload = json!({
                "invoice_id": updated.id,
                "customer_id": updated.customer_id,
                "gross_cents": updated.gross_cents,
            });
            let _ = bus.emit("billcraft", "gaincraft", "invoice.paid", payload);
        }

        Ok(updated)
    }

    pub fn flag_overdue(&self, id: &str) -> Result<Invoice, BillCoreError> {
        self.store.update_status(id, InvoiceStatus::Overdue)?;
        let updated = self.store.get_invoice(id)?;

        if let Some(bus) = &self.bus {
            // Emits to gaincraft (penalty) and taskcraft (dunning)
            let payload = json!({
                "invoice_id": updated.id,
                "customer_id": updated.customer_id,
                "customer_name": updated.customer_name,
                "account": updated.customer_id,
                "gross_cents": updated.gross_cents,
                "days_overdue": 14,
                "penalty": -25,
            });
            let _ = bus.emit("billcraft", "gaincraft", "invoice.overdue", payload.clone());
            let _ = bus.emit("billcraft", "taskcraft", "invoice.overdue", payload);
        }

        Ok(updated)
    }

    /// Drains `contract.signed` and `timesheet.approved` into draft invoices.
    ///
    /// Money fields are required. A missing amount is rejected, not replaced
    /// with a default rate or contract value. Tax on these drafts is 20%.
    pub fn drain_inbox(&self) -> Result<DrainSummary, BillCoreError> {
        let mut created = Vec::new();
        let Some(bus) = &self.bus else {
            return Ok(DrainSummary {
                processed: 0,
                created_invoices: created,
            });
        };

        let events = bus.pending("billcraft")?;
        let processed = events.len();

        for ev in events {
            let outcome = match ev.kind.as_str() {
                "contract.signed" => self.apply_contract_signed(&ev.payload),
                "timesheet.approved" => self.apply_timesheet_approved(&ev.payload),
                _ => Ok(None),
            };
            match outcome {
                Ok(Some(id)) => {
                    created.push(id);
                    bus.ack("billcraft", &ev.id)?;
                }
                Ok(None) => bus.ack("billcraft", &ev.id)?,
                Err(error) => bus.reject("billcraft", &ev.id, &error.to_string())?,
            }
        }

        Ok(DrainSummary {
            processed,
            created_invoices: created,
        })
    }

    fn apply_contract_signed(
        &self,
        payload: &serde_json::Value,
    ) -> Result<Option<String>, BillCoreError> {
        let contract_id = required_str(payload, "contract_id")?;
        let customer = required_str(payload, "customer")?;
        let value_cents = required_positive_cents(payload, "value_cents")?;
        let inv_id = format!("INV-{contract_id}");
        if self.store.get_invoice(&inv_id).is_ok() {
            return Ok(None);
        }

        let slug = customer.to_lowercase().replace(' ', "-");
        let email = optional_str(payload, "customer_email")
            .map(str::to_string)
            .unwrap_or_else(|| format!("billing@{slug}.example.invalid"));
        let today = Local::now().date_naive();
        let due = today + chrono::Days::new(14);
        self.store.create_invoice(
            &inv_id,
            &slug,
            customer,
            &email,
            None,
            None,
            None,
            today,
            due,
            Some(&format!("Generated from signed contract {contract_id}")),
        )?;
        self.store.add_item(
            &inv_id,
            "Contract Deliverable Milestone",
            1.0,
            value_cents,
            0.20,
        )?;
        Ok(Some(inv_id))
    }

    fn apply_timesheet_approved(
        &self,
        payload: &serde_json::Value,
    ) -> Result<Option<String>, BillCoreError> {
        let project = required_str(payload, "project")?;
        let hours = payload
            .get("hours")
            .and_then(serde_json::Value::as_f64)
            .filter(|hours| *hours > 0.0)
            .ok_or_else(|| BillCoreError::Validation("hours must be a positive number".into()))?;
        let rate_cents = required_positive_cents(payload, "rate_cents")?;
        let inv_id = format!("INV-TS-{project}-{}", Local::now().format("%Y%m%d"));
        if self.store.get_invoice(&inv_id).is_err() {
            let today = Local::now().date_naive();
            let due = today + chrono::Days::new(30);
            self.store.create_invoice(
                &inv_id,
                project,
                project,
                &format!("accounts@{}.example.invalid", project.to_lowercase()),
                None,
                None,
                None,
                today,
                due,
                Some(&format!("T&M billable hours for project {project}")),
            )?;
        }
        self.store.add_item(
            &inv_id,
            &format!("Billable Development ({project})"),
            hours,
            rate_cents,
            0.20,
        )?;
        Ok(Some(inv_id))
    }
}

fn required_str<'a>(payload: &'a serde_json::Value, key: &str) -> Result<&'a str, BillCoreError> {
    payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BillCoreError::Validation(format!("{key} is required")))
}

fn optional_str<'a>(payload: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn required_positive_cents(payload: &serde_json::Value, key: &str) -> Result<i64, BillCoreError> {
    let cents = payload
        .get(key)
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| BillCoreError::Validation(format!("{key} is required")))?;
    if cents <= 0 {
        return Err(BillCoreError::Validation(format!(
            "{key} must be positive cents"
        )));
    }
    Ok(cents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_biz::bus::EventState;
    use serde_json::json;
    use std::fs;

    #[test]
    fn drain_creates_drafts_and_rejects_missing_amounts() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!("billcraft-bus-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let bus = Bus::new(&root);
        bus.emit(
            "contractcraft",
            "billcraft",
            "contract.signed",
            json!({
                "contract_id": "C-9",
                "customer": "Ada GmbH",
                "value_cents": 150000
            }),
        )
        .map_err(|e| e.to_string())?;
        bus.emit(
            "teamcraft",
            "billcraft",
            "timesheet.approved",
            json!({
                "project": "Helm",
                "hours": 2.0,
                "rate_cents": 10000
            }),
        )
        .map_err(|e| e.to_string())?;
        bus.emit(
            "contractcraft",
            "billcraft",
            "contract.signed",
            json!({"contract_id": "C-missing"}),
        )
        .map_err(|e| e.to_string())?;

        let store = BillStore::open_in_memory().map_err(|e| e.to_string())?;
        let service = BillService::new(store, Some(bus));
        let summary = service.drain_inbox().map_err(|e| e.to_string())?;
        if summary.processed != 3 {
            return Err(format!("processed {}", summary.processed));
        }
        if summary.created_invoices.len() != 2 {
            return Err(format!("created {}", summary.created_invoices.len()));
        }

        let contract = service
            .store()
            .get_invoice("INV-C-9")
            .map_err(|e| e.to_string())?;
        if contract.status != InvoiceStatus::Draft || contract.gross_cents != 180_000 {
            return Err(format!("contract draft gross {}", contract.gross_cents));
        }

        let timesheet_id = summary
            .created_invoices
            .iter()
            .find(|id| id.starts_with("INV-TS-Helm-"))
            .ok_or("missing timesheet invoice")?;
        let timesheet = service
            .store()
            .get_invoice(timesheet_id)
            .map_err(|e| e.to_string())?;
        if timesheet.status != InvoiceStatus::Draft || timesheet.gross_cents != 24_000 {
            return Err(format!("timesheet draft gross {}", timesheet.gross_cents));
        }
        if service.store().get_invoice("INV-C-missing").is_ok() {
            return Err("missing amount still created an invoice".into());
        }

        let bus = service.bus().ok_or("bus dropped")?;
        let pending = bus.pending("billcraft").map_err(|e| e.to_string())?;
        if !pending.is_empty() {
            return Err(format!("inbox still has {}", pending.len()));
        }
        let failed = bus
            .history("billcraft", EventState::Failed, 10)
            .map_err(|e| e.to_string())?;
        if failed.len() != 1 {
            return Err(format!("failed {}", failed.len()));
        }

        let _ = fs::remove_dir_all(&root);
        Ok(())
    }
}
