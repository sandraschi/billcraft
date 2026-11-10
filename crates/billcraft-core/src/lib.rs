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
            return Err(BillCoreError::Validation("Cannot issue empty invoice with 0.00 total".into()));
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

    /// Drains incoming events from other craft apps (`contractcraft`, `teamcraft`, `spendcraft`)
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
            match ev.kind.as_str() {
                "contract.signed" => {
                    let contract_id = ev.payload.get("contract_id").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
                    let customer = ev.payload.get("customer").and_then(|v| v.as_str()).unwrap_or("Client");
                    let value_cents = ev.payload.get("value_cents").and_then(|v| v.as_i64()).unwrap_or(500000);
                    let inv_id = format!("INV-{}", contract_id);

                    let today = Local::now().date_naive();
                    let due = today + chrono::Days::new(14);

                    if self.store.get_invoice(&inv_id).is_err() {
                        let _ = self.store.create_invoice(
                            &inv_id,
                            &customer.to_lowercase().replace(' ', "-"),
                            customer,
                            &format!("billing@{}", customer.to_lowercase().replace(' ', "-")),
                            None,
                            None,
                            None,
                            today,
                            due,
                            Some(&format!("Generated from signed contract {contract_id}")),
                        );
                        let _ = self.store.add_item(&inv_id, "Contract Deliverable Milestone", 1.0, value_cents, 0.20);
                        created.push(inv_id);
                    }
                }
                "timesheet.approved" => {
                    let project = ev.payload.get("project").and_then(|v| v.as_str()).unwrap_or("PROJ");
                    let hours = ev.payload.get("hours").and_then(|v| v.as_f64()).unwrap_or(8.0);
                    let rate_cents = ev.payload.get("rate_cents").and_then(|v| v.as_i64()).unwrap_or(12000);
                    let inv_id = format!("INV-TS-{}-{}", project, Local::now().format("%Y%m%d"));

                    let today = Local::now().date_naive();
                    let due = today + chrono::Days::new(30);

                    if self.store.get_invoice(&inv_id).is_err() {
                        let _ = self.store.create_invoice(
                            &inv_id,
                            project,
                            project,
                            &format!("accounts@{}", project.to_lowercase()),
                            None,
                            None,
                            None,
                            today,
                            due,
                            Some(&format!("T&M billable hours for project {project}")),
                        );
                    }
                    let _ = self.store.add_item(&inv_id, &format!("Billable Development ({project})"), hours, rate_cents, 0.20);
                    created.push(inv_id);
                }
                _ => {}
            }
            let _ = bus.ack("billcraft", &ev.id);
        }

        Ok(DrainSummary {
            processed,
            created_invoices: created,
        })
    }
}
