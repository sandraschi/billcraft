#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BillStoreError {
    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invoice not found: {0}")]
    NotFound(String),
    #[error("Invalid status transition: {0}")]
    InvalidStatus(String),
    #[error("Validation error: {0}")]
    Validation(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceStatus {
    Draft,
    Issued,
    Paid,
    Overdue,
    Canceled,
}

impl InvoiceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Issued => "issued",
            Self::Paid => "paid",
            Self::Overdue => "overdue",
            Self::Canceled => "canceled",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "issued" => Some(Self::Issued),
            "paid" => Some(Self::Paid),
            "overdue" => Some(Self::Overdue),
            "canceled" => Some(Self::Canceled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceItem {
    pub id: String,
    pub invoice_id: String,
    pub position: i32,
    pub description: String,
    pub quantity: f64,
    pub unit_price_cents: i64,
    pub tax_rate: f64,
    pub total_cents: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invoice {
    pub id: String,
    pub customer_id: String,
    pub customer_name: String,
    pub customer_email: String,
    pub customer_uid: Option<String>,
    pub customer_iban: Option<String>,
    pub customer_bic: Option<String>,
    pub status: InvoiceStatus,
    pub issue_date: NaiveDate,
    pub due_date: NaiveDate,
    pub currency: String,
    pub net_cents: i64,
    pub tax_cents: i64,
    pub gross_cents: i64,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub customer_id: String,
    pub plan_name: String,
    pub interval_months: i32,
    pub amount_cents: i64,
    pub next_billing_date: NaiveDate,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillStats {
    pub total_invoices: usize,
    pub total_net_cents: i64,
    pub total_tax_cents: i64,
    pub total_gross_cents: i64,
    pub paid_gross_cents: i64,
    pub overdue_gross_cents: i64,
    pub mrr_cents: i64,
}

pub struct BillStore {
    conn: Connection,
}

impl BillStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, BillStoreError> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, BillStoreError> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), BillStoreError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS invoices (
                id TEXT PRIMARY KEY,
                customer_id TEXT NOT NULL,
                customer_name TEXT NOT NULL,
                customer_email TEXT NOT NULL,
                customer_uid TEXT,
                customer_iban TEXT,
                customer_bic TEXT,
                status TEXT NOT NULL,
                issue_date TEXT NOT NULL,
                due_date TEXT NOT NULL,
                currency TEXT NOT NULL,
                net_cents INTEGER NOT NULL,
                tax_cents INTEGER NOT NULL,
                gross_cents INTEGER NOT NULL,
                notes TEXT,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_invoices_status ON invoices(status);
            CREATE INDEX IF NOT EXISTS idx_invoices_customer ON invoices(customer_id);

            CREATE TABLE IF NOT EXISTS invoice_items (
                id TEXT PRIMARY KEY,
                invoice_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                description TEXT NOT NULL,
                quantity REAL NOT NULL,
                unit_price_cents INTEGER NOT NULL,
                tax_rate REAL NOT NULL,
                total_cents INTEGER NOT NULL,
                FOREIGN KEY (invoice_id) REFERENCES invoices(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_items_invoice ON invoice_items(invoice_id);

            CREATE TABLE IF NOT EXISTS xrechnung_party (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                street TEXT NOT NULL,
                city TEXT NOT NULL,
                postcode TEXT NOT NULL,
                country TEXT NOT NULL,
                email TEXT NOT NULL,
                vat_id TEXT,
                phone TEXT,
                iban TEXT,
                buyer_reference TEXT
            );

            CREATE TABLE IF NOT EXISTS subscriptions (
                id TEXT PRIMARY KEY,
                customer_id TEXT NOT NULL,
                plan_name TEXT NOT NULL,
                interval_months INTEGER NOT NULL,
                amount_cents INTEGER NOT NULL,
                next_billing_date TEXT NOT NULL,
                status TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_invoice(
        &self,
        id: &str,
        customer_id: &str,
        customer_name: &str,
        customer_email: &str,
        customer_uid: Option<&str>,
        customer_iban: Option<&str>,
        customer_bic: Option<&str>,
        issue_date: NaiveDate,
        due_date: NaiveDate,
        notes: Option<&str>,
    ) -> Result<Invoice, BillStoreError> {
        let now = Utc::now();
        let inv = Invoice {
            id: id.to_string(),
            customer_id: customer_id.to_string(),
            customer_name: customer_name.to_string(),
            customer_email: customer_email.to_string(),
            customer_uid: customer_uid.map(|s| s.to_string()),
            customer_iban: customer_iban.map(|s| s.to_string()),
            customer_bic: customer_bic.map(|s| s.to_string()),
            status: InvoiceStatus::Draft,
            issue_date,
            due_date,
            currency: "EUR".to_string(),
            net_cents: 0,
            tax_cents: 0,
            gross_cents: 0,
            notes: notes.map(|s| s.to_string()),
            created_at: now,
        };

        self.conn.execute(
            "INSERT INTO invoices (
                id, customer_id, customer_name, customer_email, customer_uid, customer_iban,
                customer_bic, status, issue_date, due_date, currency, net_cents, tax_cents,
                gross_cents, notes, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                inv.id,
                inv.customer_id,
                inv.customer_name,
                inv.customer_email,
                inv.customer_uid,
                inv.customer_iban,
                inv.customer_bic,
                inv.status.as_str(),
                inv.issue_date.to_string(),
                inv.due_date.to_string(),
                inv.currency,
                inv.net_cents,
                inv.tax_cents,
                inv.gross_cents,
                inv.notes,
                inv.created_at.to_rfc3339()
            ],
        )?;

        Ok(inv)
    }

    pub fn add_item(
        &self,
        invoice_id: &str,
        description: &str,
        quantity: f64,
        unit_price_cents: i64,
        tax_rate: f64,
    ) -> Result<InvoiceItem, BillStoreError> {
        let item_id = format!(
            "{invoice_id}-item-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        let total_cents = (quantity * unit_price_cents as f64).round() as i64;

        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM invoice_items WHERE invoice_id = ?1",
        )?;
        let pos: i32 = stmt.query_row(params![invoice_id], |row| row.get(0))?;

        let item = InvoiceItem {
            id: item_id,
            invoice_id: invoice_id.to_string(),
            position: pos,
            description: description.to_string(),
            quantity,
            unit_price_cents,
            tax_rate,
            total_cents,
        };

        self.conn.execute(
            "INSERT INTO invoice_items (id, invoice_id, position, description, quantity, unit_price_cents, tax_rate, total_cents)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                item.id,
                item.invoice_id,
                item.position,
                item.description,
                item.quantity,
                item.unit_price_cents,
                item.tax_rate,
                item.total_cents
            ],
        )?;

        self.recalculate_invoice_totals(invoice_id)?;
        Ok(item)
    }

    fn recalculate_invoice_totals(&self, invoice_id: &str) -> Result<(), BillStoreError> {
        let items = self.get_items(invoice_id)?;
        let mut net = 0i64;
        let mut tax = 0i64;

        for it in items {
            net += it.total_cents;
            let item_tax = (it.total_cents as f64 * it.tax_rate).round() as i64;
            tax += item_tax;
        }
        let gross = net + tax;

        self.conn.execute(
            "UPDATE invoices SET net_cents = ?1, tax_cents = ?2, gross_cents = ?3 WHERE id = ?4",
            params![net, tax, gross, invoice_id],
        )?;
        Ok(())
    }

    pub fn get_invoice(&self, id: &str) -> Result<Invoice, BillStoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, customer_id, customer_name, customer_email, customer_uid, customer_iban,
                    customer_bic, status, issue_date, due_date, currency, net_cents, tax_cents,
                    gross_cents, notes, created_at
             FROM invoices WHERE id = ?1",
        )?;

        stmt.query_row(params![id], |row| {
            let status_str: String = row.get(7)?;
            let status = InvoiceStatus::from_str_opt(&status_str).unwrap_or(InvoiceStatus::Draft);
            let issue_str: String = row.get(8)?;
            let due_str: String = row.get(9)?;
            let created_str: String = row.get(15)?;

            let issue_date = NaiveDate::parse_from_str(&issue_str, "%Y-%m-%d")
                .unwrap_or_else(|_| NaiveDate::from_ymd_opt(2026, 1, 1).unwrap_or_default());
            let due_date = NaiveDate::parse_from_str(&due_str, "%Y-%m-%d")
                .unwrap_or_else(|_| NaiveDate::from_ymd_opt(2026, 1, 1).unwrap_or_default());
            let created_at = DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(Invoice {
                id: row.get(0)?,
                customer_id: row.get(1)?,
                customer_name: row.get(2)?,
                customer_email: row.get(3)?,
                customer_uid: row.get(4)?,
                customer_iban: row.get(5)?,
                customer_bic: row.get(6)?,
                status,
                issue_date,
                due_date,
                currency: row.get(10)?,
                net_cents: row.get(11)?,
                tax_cents: row.get(12)?,
                gross_cents: row.get(13)?,
                notes: row.get(14)?,
                created_at,
            })
        })
        .optional()?
        .ok_or_else(|| BillStoreError::NotFound(id.to_string()))
    }

    pub fn list_invoices(
        &self,
        status: Option<InvoiceStatus>,
    ) -> Result<Vec<Invoice>, BillStoreError> {
        let mut sql = "SELECT id, customer_id, customer_name, customer_email, customer_uid, customer_iban,
                              customer_bic, status, issue_date, due_date, currency, net_cents, tax_cents,
                              gross_cents, notes, created_at
                       FROM invoices".to_string();
        if status.is_some() {
            sql.push_str(" WHERE status = ?1");
        }
        sql.push_str(" ORDER BY issue_date DESC, id DESC");

        let mut stmt = self.conn.prepare(&sql)?;
        let mut out = Vec::new();
        if let Some(st) = status {
            let rows = stmt.query_map(params![st.as_str()], Self::row_to_invoice)?;
            for r in rows {
                out.push(r?);
            }
        } else {
            let rows = stmt.query_map([], Self::row_to_invoice)?;
            for r in rows {
                out.push(r?);
            }
        }
        Ok(out)
    }

    fn row_to_invoice(row: &rusqlite::Row) -> Result<Invoice, rusqlite::Error> {
        let status_str: String = row.get(7)?;
        let status = InvoiceStatus::from_str_opt(&status_str).unwrap_or(InvoiceStatus::Draft);
        let issue_str: String = row.get(8)?;
        let due_str: String = row.get(9)?;
        let created_str: String = row.get(15)?;

        let issue_date = NaiveDate::parse_from_str(&issue_str, "%Y-%m-%d")
            .unwrap_or_else(|_| NaiveDate::from_ymd_opt(2026, 1, 1).unwrap_or_default());
        let due_date = NaiveDate::parse_from_str(&due_str, "%Y-%m-%d")
            .unwrap_or_else(|_| NaiveDate::from_ymd_opt(2026, 1, 1).unwrap_or_default());
        let created_at = DateTime::parse_from_rfc3339(&created_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        Ok(Invoice {
            id: row.get(0)?,
            customer_id: row.get(1)?,
            customer_name: row.get(2)?,
            customer_email: row.get(3)?,
            customer_uid: row.get(4)?,
            customer_iban: row.get(5)?,
            customer_bic: row.get(6)?,
            status,
            issue_date,
            due_date,
            currency: row.get(10)?,
            net_cents: row.get(11)?,
            tax_cents: row.get(12)?,
            gross_cents: row.get(13)?,
            notes: row.get(14)?,
            created_at,
        })
    }

    pub fn update_status(&self, id: &str, new_status: InvoiceStatus) -> Result<(), BillStoreError> {
        let rows = self.conn.execute(
            "UPDATE invoices SET status = ?1 WHERE id = ?2",
            params![new_status.as_str(), id],
        )?;
        if rows == 0 {
            return Err(BillStoreError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn get_items(&self, invoice_id: &str) -> Result<Vec<InvoiceItem>, BillStoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, invoice_id, position, description, quantity, unit_price_cents, tax_rate, total_cents
             FROM invoice_items WHERE invoice_id = ?1 ORDER BY position ASC",
        )?;

        let rows = stmt.query_map(params![invoice_id], |row| {
            Ok(InvoiceItem {
                id: row.get(0)?,
                invoice_id: row.get(1)?,
                position: row.get(2)?,
                description: row.get(3)?,
                quantity: row.get(4)?,
                unit_price_cents: row.get(5)?,
                tax_rate: row.get(6)?,
                total_cents: row.get(7)?,
            })
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn create_subscription(
        &self,
        id: &str,
        customer_id: &str,
        plan_name: &str,
        interval_months: i32,
        amount_cents: i64,
        next_billing_date: NaiveDate,
    ) -> Result<Subscription, BillStoreError> {
        let sub = Subscription {
            id: id.to_string(),
            customer_id: customer_id.to_string(),
            plan_name: plan_name.to_string(),
            interval_months,
            amount_cents,
            next_billing_date,
            status: "active".to_string(),
        };

        self.conn.execute(
            "INSERT INTO subscriptions (id, customer_id, plan_name, interval_months, amount_cents, next_billing_date, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                sub.id,
                sub.customer_id,
                sub.plan_name,
                sub.interval_months,
                sub.amount_cents,
                sub.next_billing_date.to_string(),
                sub.status
            ],
        )?;

        Ok(sub)
    }

    pub fn list_subscriptions(&self) -> Result<Vec<Subscription>, BillStoreError> {
        let mut stmt = self.conn.prepare("SELECT id, customer_id, plan_name, interval_months, amount_cents, next_billing_date, status FROM subscriptions")?;
        let rows = stmt.query_map([], |row| {
            let next_str: String = row.get(5)?;
            let next_billing_date = NaiveDate::parse_from_str(&next_str, "%Y-%m-%d")
                .unwrap_or_else(|_| NaiveDate::from_ymd_opt(2026, 1, 1).unwrap_or_default());
            Ok(Subscription {
                id: row.get(0)?,
                customer_id: row.get(1)?,
                plan_name: row.get(2)?,
                interval_months: row.get(3)?,
                amount_cents: row.get(4)?,
                next_billing_date,
                status: row.get(6)?,
            })
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn stats(&self) -> Result<BillStats, BillStoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT 
                COUNT(*),
                COALESCE(SUM(net_cents), 0),
                COALESCE(SUM(tax_cents), 0),
                COALESCE(SUM(gross_cents), 0),
                COALESCE(SUM(CASE WHEN status = 'paid' THEN gross_cents ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'overdue' THEN gross_cents ELSE 0 END), 0)
             FROM invoices",
        )?;

        let (
            total_invoices,
            total_net_cents,
            total_tax_cents,
            total_gross_cents,
            paid_gross_cents,
            overdue_gross_cents,
        ) = stmt.query_row([], |row| {
            Ok((
                row.get::<_, i64>(0)? as usize,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;

        let mut sub_stmt = self.conn.prepare(
            "SELECT COALESCE(SUM(CASE WHEN interval_months = 1 THEN amount_cents WHEN interval_months = 12 THEN amount_cents / 12 ELSE amount_cents END), 0)
             FROM subscriptions WHERE status = 'active'"
        )?;
        let mrr_cents: i64 = sub_stmt.query_row([], |row| row.get(0))?;

        Ok(BillStats {
            total_invoices,
            total_net_cents,
            total_tax_cents,
            total_gross_cents,
            paid_gross_cents,
            overdue_gross_cents,
            mrr_cents,
        })
    }

    /// Austrian ebInterface 6.1 generator for sovereign federal/B2B invoicing.
    pub fn export_ebinterface(&self, invoice_id: &str) -> Result<String, BillStoreError> {
        let inv = self.get_invoice(invoice_id)?;
        let items = self.get_items(invoice_id)?;

        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str("<eb:Invoice xmlns:eb=\"http://www.ebinterface.at/schema/6p1/\"\n");
        xml.push_str("            eb:GeneratingSystem=\"BillCraft Local-First Engine\"\n");
        xml.push_str("            eb:DocumentType=\"Invoice\"\n");
        xml.push_str(&format!("            eb:InvoiceNumber=\"{}\"\n", inv.id));
        xml.push_str(&format!(
            "            eb:InvoiceDate=\"{}\">\n",
            inv.issue_date
        ));

        // Invoice Recipient
        xml.push_str("  <eb:InvoiceRecipient>\n");
        if let Some(uid) = &inv.customer_uid {
            xml.push_str(&format!(
                "    <eb:VATIdentificationNumber>{uid}</eb:VATIdentificationNumber>\n"
            ));
        }
        xml.push_str("    <eb:OrderReference>\n");
        xml.push_str(&format!(
            "      <eb:OrderID>ORDER-{}</eb:OrderID>\n",
            inv.id
        ));
        xml.push_str("    </eb:OrderReference>\n");
        xml.push_str("    <eb:Address>\n");
        xml.push_str(&format!("      <eb:Name>{}</eb:Name>\n", inv.customer_name));
        xml.push_str(&format!(
            "      <eb:Email>{}</eb:Email>\n",
            inv.customer_email
        ));
        xml.push_str("    </eb:Address>\n");
        xml.push_str("  </eb:InvoiceRecipient>\n");

        // Line details
        xml.push_str("  <eb:Details>\n");
        xml.push_str("    <eb:ItemList>\n");
        for it in &items {
            xml.push_str("      <eb:ListLineItem>\n");
            xml.push_str(&format!(
                "        <eb:PositionNumber>{}</eb:PositionNumber>\n",
                it.position
            ));
            xml.push_str(&format!(
                "        <eb:Description>{}</eb:Description>\n",
                it.description
            ));
            xml.push_str(&format!(
                "        <eb:Quantity eb:Unit=\"HUR\">{:.2}</eb:Quantity>\n",
                it.quantity
            ));
            xml.push_str(&format!(
                "        <eb:UnitPrice>{:.2}</eb:UnitPrice>\n",
                it.unit_price_cents as f64 / 100.0
            ));
            xml.push_str(&format!(
                "        <eb:TaxRate>{:.1}</eb:TaxRate>\n",
                it.tax_rate * 100.0
            ));
            xml.push_str(&format!(
                "        <eb:LineItemAmount>{:.2}</eb:LineItemAmount>\n",
                it.total_cents as f64 / 100.0
            ));
            xml.push_str("      </eb:ListLineItem>\n");
        }
        xml.push_str("    </eb:ItemList>\n");
        xml.push_str("  </eb:Details>\n");

        // Tax
        xml.push_str("  <eb:Tax>\n");
        xml.push_str("    <eb:VAT>\n");
        xml.push_str("      <eb:VATItem>\n");
        xml.push_str("        <eb:TaxedAmount>");
        xml.push_str(&format!("{:.2}", inv.net_cents as f64 / 100.0));
        xml.push_str("</eb:TaxedAmount>\n");
        xml.push_str("        <eb:TaxRate>20.0</eb:TaxRate>\n");
        xml.push_str("        <eb:Amount>");
        xml.push_str(&format!("{:.2}", inv.tax_cents as f64 / 100.0));
        xml.push_str("</eb:Amount>\n");
        xml.push_str("      </eb:VATItem>\n");
        xml.push_str("    </eb:VAT>\n");
        xml.push_str("  </eb:Tax>\n");

        // Total
        xml.push_str(&format!(
            "  <eb:TotalGrossAmount>{:.2}</eb:TotalGrossAmount>\n",
            inv.gross_cents as f64 / 100.0
        ));
        xml.push_str(&format!(
            "  <eb:PayableAmount>{:.2}</eb:PayableAmount>\n",
            inv.gross_cents as f64 / 100.0
        ));
        xml.push_str("</eb:Invoice>\n");

        Ok(xml)
    }

    /// Generates ISO 20022 pain.008.001.02 XML for direct bank direct debit collection.
    pub fn export_sepa_direct_debit(
        &self,
        invoice_ids: &[String],
        creditor_name: &str,
        creditor_iban: &str,
        creditor_bic: &str,
        creditor_id: &str,
    ) -> Result<String, BillStoreError> {
        let mut invoices = Vec::new();
        for id in invoice_ids {
            invoices.push(self.get_invoice(id)?);
        }

        let total_cents: i64 = invoices.iter().map(|i| i.gross_cents).sum();
        let count = invoices.len();
        let msg_id = format!("SEPA-DD-{}", Utc::now().format("%Y%m%d%H%M%S"));

        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str("<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:pain.008.001.02\">\n");
        xml.push_str("  <CstmrDrctDbtInitn>\n");
        xml.push_str("    <GrpHdr>\n");
        xml.push_str(&format!("      <MsgId>{msg_id}</MsgId>\n"));
        xml.push_str(&format!(
            "      <CreDtTm>{}</CreDtTm>\n",
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ")
        ));
        xml.push_str(&format!("      <NbOfTxs>{count}</NbOfTxs>\n"));
        xml.push_str(&format!(
            "      <CtrlSum>{:.2}</CtrlSum>\n",
            total_cents as f64 / 100.0
        ));
        xml.push_str("      <InitgPty>\n");
        xml.push_str(&format!("        <Nm>{creditor_name}</Nm>\n"));
        xml.push_str("      </InitgPty>\n");
        xml.push_str("    </GrpHdr>\n");

        xml.push_str("    <PmtInf>\n");
        xml.push_str(&format!("      <PmtInfId>{msg_id}-01</PmtInfId>\n"));
        xml.push_str("      <PmtMtd>DD</PmtMtd>\n");
        xml.push_str("      <PmtTpInf>\n");
        xml.push_str("        <SvcLvl><Cd>SEPA</Cd></SvcLvl>\n");
        xml.push_str("        <LclInstrm><Cd>CORE</Cd></LclInstrm>\n");
        xml.push_str("        <SeqTp>RCUR</SeqTp>\n");
        xml.push_str("      </PmtTpInf>\n");
        xml.push_str(&format!(
            "      <ReqdColltnDt>{}</ReqdColltnDt>\n",
            Utc::now().date_naive()
        ));
        xml.push_str("      <Cdtr>\n");
        xml.push_str(&format!("        <Nm>{creditor_name}</Nm>\n"));
        xml.push_str("      </Cdtr>\n");
        xml.push_str("      <CdtrAcct>\n");
        xml.push_str("        <Id><IBAN>");
        xml.push_str(creditor_iban);
        xml.push_str("</IBAN></Id>\n");
        xml.push_str("      </CdtrAcct>\n");
        xml.push_str("      <CdtrAgt>\n");
        xml.push_str("        <FinInstnId><BIC>");
        xml.push_str(creditor_bic);
        xml.push_str("</BIC></FinInstnId>\n");
        xml.push_str("      </CdtrAgt>\n");
        xml.push_str("      <CdtrSchmeId>\n");
        xml.push_str("        <Id><SpcltySchemeId><Id>");
        xml.push_str(creditor_id);
        xml.push_str("</Id></SpcltySchemeId></Id>\n");
        xml.push_str("      </CdtrSchmeId>\n");

        for inv in invoices {
            xml.push_str("      <DrctDbtTxInf>\n");
            xml.push_str("        <PmtId>\n");
            xml.push_str(&format!("          <EndToEndId>{}</EndToEndId>\n", inv.id));
            xml.push_str("        </PmtId>\n");
            xml.push_str(&format!(
                "        <InstdAmt Ccy=\"EUR\">{:.2}</InstdAmt>\n",
                inv.gross_cents as f64 / 100.0
            ));
            xml.push_str("        <DrctDbtTx>\n");
            xml.push_str("          <MndtRltdInf>\n");
            xml.push_str(&format!(
                "            <MndtId>MANDATE-{}</MndtId>\n",
                inv.customer_id
            ));
            xml.push_str(&format!(
                "            <DtOfSgntr>{}</DtOfSgntr>\n",
                inv.issue_date
            ));
            xml.push_str("          </MndtRltdInf>\n");
            xml.push_str("        </DrctDbtTx>\n");
            xml.push_str("        <DbtrAgt>\n");
            xml.push_str(&format!(
                "          <FinInstnId><BIC>{}</BIC></FinInstnId>\n",
                inv.customer_bic.as_deref().unwrap_or("NOTPROVIDED")
            ));
            xml.push_str("        </DbtrAgt>\n");
            xml.push_str("        <Dbtr>\n");
            xml.push_str(&format!("          <Nm>{}</Nm>\n", inv.customer_name));
            xml.push_str("        </Dbtr>\n");
            xml.push_str("        <DbtrAcct>\n");
            xml.push_str(&format!(
                "          <Id><IBAN>{}</IBAN></Id>\n",
                inv.customer_iban.as_deref().unwrap_or("NOTPROVIDED")
            ));
            xml.push_str("        </DbtrAcct>\n");
            xml.push_str("        <RmtInf>\n");
            xml.push_str(&format!("          <Ustrd>Invoice {}</Ustrd>\n", inv.id));
            xml.push_str("        </RmtInf>\n");
            xml.push_str("      </DrctDbtTxInf>\n");
        }

        xml.push_str("    </PmtInf>\n");
        xml.push_str("  </CstmrDrctDbtInitn>\n");
        xml.push_str("</Document>\n");

        Ok(xml)
    }

    pub fn save_xrechnung_party(&self, id: &str, party: &XrParty) -> Result<(), BillStoreError> {
        if party.country.len() != 2 {
            return Err(BillStoreError::Validation(
                "XRechnung country is a two-letter code".into(),
            ));
        }
        self.conn.execute(
            "INSERT INTO xrechnung_party (
                id, name, street, city, postcode, country, email, vat_id, phone, iban, buyer_reference
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                street = excluded.street,
                city = excluded.city,
                postcode = excluded.postcode,
                country = excluded.country,
                email = excluded.email,
                vat_id = excluded.vat_id,
                phone = excluded.phone,
                iban = excluded.iban,
                buyer_reference = excluded.buyer_reference",
            params![
                id,
                party.name,
                party.street,
                party.city,
                party.postcode,
                party.country,
                party.email,
                party.vat_id,
                party.phone,
                party.iban,
                party.buyer_reference
            ],
        )?;
        Ok(())
    }

    fn xrechnung_party(&self, id: &str) -> Result<XrParty, BillStoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT name, street, city, postcode, country, email, vat_id, phone, iban, buyer_reference
             FROM xrechnung_party WHERE id = ?1",
        )?;
        let party = stmt
            .query_row(params![id], |row| {
                Ok(XrParty {
                    name: row.get(0)?,
                    street: row.get(1)?,
                    city: row.get(2)?,
                    postcode: row.get(3)?,
                    country: row.get(4)?,
                    email: row.get(5)?,
                    vat_id: row.get(6)?,
                    phone: row.get(7)?,
                    iban: row.get(8)?,
                    buyer_reference: row.get(9)?,
                })
            })
            .optional()?
            .ok_or_else(|| {
                BillStoreError::Validation(format!("XRechnung party {id} is missing"))
            })?;
        Ok(party)
    }

    /// UBL 2.1 invoice for XRechnung 3.0.2.
    /// Seller is party id `seller`. Buyer is party id equal to the invoice id.
    /// A missing party is an error. This is not a Peppol transmission.
    pub fn export_xrechnung(&self, invoice_id: &str) -> Result<String, BillStoreError> {
        let inv = self.get_invoice(invoice_id)?;
        let items = self.get_items(invoice_id)?;
        if items.is_empty() {
            return Err(BillStoreError::Validation(
                "XRechnung needs at least one invoice line".into(),
            ));
        }
        let seller = self.xrechnung_party("seller")?;
        let buyer = self.xrechnung_party(invoice_id)?;
        let seller_vat = seller
            .vat_id
            .as_deref()
            .ok_or_else(|| BillStoreError::Validation("XRechnung seller needs a VAT id".into()))?;
        let seller_phone = seller.phone.as_deref().ok_or_else(|| {
            BillStoreError::Validation("XRechnung seller needs a phone number".into())
        })?;
        let seller_iban = seller
            .iban
            .as_deref()
            .ok_or_else(|| BillStoreError::Validation("XRechnung seller needs an IBAN".into()))?;
        let buyer_ref = buyer.buyer_reference.as_deref().ok_or_else(|| {
            BillStoreError::Validation("XRechnung buyer needs a buyer reference".into())
        })?;
        let currency = &inv.currency;
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str("<ubl:Invoice xmlns:ubl=\"urn:oasis:names:specification:ubl:schema:xsd:Invoice-2\" xmlns:cac=\"urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2\" xmlns:cbc=\"urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2\">\n");
        xml.push_str("  <cbc:CustomizationID>urn:cen.eu:en16931:2017#compliant#urn:xeinkauf.de:kosit:xrechnung_3.0</cbc:CustomizationID>\n");
        xml.push_str(
            "  <cbc:ProfileID>urn:fdc:peppol.eu:2017:poacc:billing:01:1.0</cbc:ProfileID>\n",
        );
        xml.push_str(&format!("  <cbc:ID>{}</cbc:ID>\n", xml_escape(&inv.id)));
        xml.push_str(&format!(
            "  <cbc:IssueDate>{}</cbc:IssueDate>\n",
            inv.issue_date
        ));
        xml.push_str(&format!("  <cbc:DueDate>{}</cbc:DueDate>\n", inv.due_date));
        xml.push_str("  <cbc:InvoiceTypeCode>380</cbc:InvoiceTypeCode>\n");
        xml.push_str(&format!(
            "  <cbc:DocumentCurrencyCode>{}</cbc:DocumentCurrencyCode>\n",
            xml_escape(currency)
        ));
        xml.push_str(&format!(
            "  <cbc:BuyerReference>{}</cbc:BuyerReference>\n",
            xml_escape(buyer_ref)
        ));
        xml.push_str("  <cac:InvoicePeriod>\n");
        xml.push_str(&format!(
            "    <cbc:StartDate>{}</cbc:StartDate>\n",
            inv.issue_date
        ));
        xml.push_str(&format!(
            "    <cbc:EndDate>{}</cbc:EndDate>\n",
            inv.due_date
        ));
        xml.push_str("  </cac:InvoicePeriod>\n");
        xml.push_str(&party_xml(
            "AccountingSupplierParty",
            &seller,
            true,
            seller_vat,
            seller_phone,
        ));
        xml.push_str(&party_xml("AccountingCustomerParty", &buyer, false, "", ""));
        xml.push_str("  <cac:PaymentMeans>\n");
        xml.push_str("    <cbc:PaymentMeansCode>58</cbc:PaymentMeansCode>\n");
        xml.push_str("    <cac:PayeeFinancialAccount>\n");
        xml.push_str(&format!(
            "      <cbc:ID>{}</cbc:ID>\n",
            xml_escape(seller_iban)
        ));
        xml.push_str("    </cac:PayeeFinancialAccount>\n");
        xml.push_str("  </cac:PaymentMeans>\n");
        xml.push_str(&tax_and_totals(&inv, &items));
        for it in &items {
            let line = money(it.total_cents);
            let price = money(it.unit_price_cents);
            let percent = tax_percent(it.tax_rate);
            let qty = format_qty(it.quantity);
            xml.push_str("  <cac:InvoiceLine>\n");
            xml.push_str(&format!("    <cbc:ID>{}</cbc:ID>\n", it.position));
            xml.push_str(&format!(
                "    <cbc:InvoicedQuantity unitCode=\"HUR\">{qty}</cbc:InvoicedQuantity>\n"
            ));
            xml.push_str(&format!(
                "    <cbc:LineExtensionAmount currencyID=\"{currency}\">{line}</cbc:LineExtensionAmount>\n"
            ));
            xml.push_str("    <cac:Item>\n");
            xml.push_str(&format!(
                "      <cbc:Name>{}</cbc:Name>\n",
                xml_escape(&it.description)
            ));
            xml.push_str("      <cac:ClassifiedTaxCategory>\n");
            xml.push_str("        <cbc:ID>S</cbc:ID>\n");
            xml.push_str(&format!("        <cbc:Percent>{percent}</cbc:Percent>\n"));
            xml.push_str("        <cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme>\n");
            xml.push_str("      </cac:ClassifiedTaxCategory>\n");
            xml.push_str("    </cac:Item>\n");
            xml.push_str("    <cac:Price>\n");
            xml.push_str(&format!(
                "      <cbc:PriceAmount currencyID=\"{currency}\">{price}</cbc:PriceAmount>\n"
            ));
            xml.push_str("    </cac:Price>\n");
            xml.push_str("  </cac:InvoiceLine>\n");
        }
        xml.push_str("</ubl:Invoice>\n");
        Ok(xml)
    }
}

#[derive(Debug, Clone)]
pub struct XrParty {
    pub name: String,
    pub street: String,
    pub city: String,
    pub postcode: String,
    pub country: String,
    pub email: String,
    pub vat_id: Option<String>,
    pub phone: Option<String>,
    pub iban: Option<String>,
    pub buyer_reference: Option<String>,
}

fn money(cents: i64) -> String {
    format!("{:.2}", cents as f64 / 100.0)
}

fn tax_percent(rate: f64) -> String {
    let percent = (rate * 100.0).round();
    if percent.fract() == 0.0 {
        format!("{}", percent as i64)
    } else {
        format!("{percent}")
    }
}

fn format_qty(quantity: f64) -> String {
    if quantity.fract() == 0.0 {
        format!("{}", quantity as i64)
    } else {
        format!("{quantity}")
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn party_xml(tag: &str, party: &XrParty, seller: bool, vat_id: &str, phone: &str) -> String {
    let mut xml = format!("  <cac:{tag}>\n    <cac:Party>\n");
    xml.push_str(&format!(
        "      <cbc:EndpointID schemeID=\"EM\">{}</cbc:EndpointID>\n",
        xml_escape(&party.email)
    ));
    xml.push_str("      <cac:PostalAddress>\n");
    xml.push_str(&format!(
        "        <cbc:StreetName>{}</cbc:StreetName>\n",
        xml_escape(&party.street)
    ));
    xml.push_str(&format!(
        "        <cbc:CityName>{}</cbc:CityName>\n",
        xml_escape(&party.city)
    ));
    xml.push_str(&format!(
        "        <cbc:PostalZone>{}</cbc:PostalZone>\n",
        xml_escape(&party.postcode)
    ));
    xml.push_str("        <cac:Country>\n");
    xml.push_str(&format!(
        "          <cbc:IdentificationCode>{}</cbc:IdentificationCode>\n",
        xml_escape(&party.country)
    ));
    xml.push_str("        </cac:Country>\n");
    xml.push_str("      </cac:PostalAddress>\n");
    if seller {
        xml.push_str("      <cac:PartyTaxScheme>\n");
        xml.push_str(&format!(
            "        <cbc:CompanyID>{}</cbc:CompanyID>\n",
            xml_escape(vat_id)
        ));
        xml.push_str("        <cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme>\n");
        xml.push_str("      </cac:PartyTaxScheme>\n");
    }
    xml.push_str("      <cac:PartyLegalEntity>\n");
    xml.push_str(&format!(
        "        <cbc:RegistrationName>{}</cbc:RegistrationName>\n",
        xml_escape(&party.name)
    ));
    xml.push_str("      </cac:PartyLegalEntity>\n");
    if seller {
        xml.push_str("      <cac:Contact>\n");
        xml.push_str(&format!(
            "        <cbc:Name>{}</cbc:Name>\n",
            xml_escape(&party.name)
        ));
        xml.push_str(&format!(
            "        <cbc:Telephone>{}</cbc:Telephone>\n",
            xml_escape(phone)
        ));
        xml.push_str(&format!(
            "        <cbc:ElectronicMail>{}</cbc:ElectronicMail>\n",
            xml_escape(&party.email)
        ));
        xml.push_str("      </cac:Contact>\n");
    }
    xml.push_str("    </cac:Party>\n");
    xml.push_str(&format!("  </cac:{tag}>\n"));
    xml
}

fn tax_and_totals(inv: &Invoice, items: &[InvoiceItem]) -> String {
    let currency = &inv.currency;
    let mut groups: Vec<(i64, i64, i64)> = Vec::new();
    for item in items {
        let percent = (item.tax_rate * 100.0).round() as i64;
        let tax = (item.total_cents as f64 * item.tax_rate).round() as i64;
        if let Some(group) = groups.iter_mut().find(|group| group.0 == percent) {
            group.1 += item.total_cents;
            group.2 += tax;
        } else {
            groups.push((percent, item.total_cents, tax));
        }
    }
    let mut xml = String::new();
    xml.push_str("  <cac:TaxTotal>\n");
    xml.push_str(&format!(
        "    <cbc:TaxAmount currencyID=\"{currency}\">{}</cbc:TaxAmount>\n",
        money(inv.tax_cents)
    ));
    for (percent, taxable, tax) in &groups {
        xml.push_str("    <cac:TaxSubtotal>\n");
        xml.push_str(&format!(
            "      <cbc:TaxableAmount currencyID=\"{currency}\">{}</cbc:TaxableAmount>\n",
            money(*taxable)
        ));
        xml.push_str(&format!(
            "      <cbc:TaxAmount currencyID=\"{currency}\">{tax_money}</cbc:TaxAmount>\n",
            tax_money = money(*tax)
        ));
        xml.push_str("      <cac:TaxCategory>\n");
        xml.push_str("        <cbc:ID>S</cbc:ID>\n");
        xml.push_str(&format!("        <cbc:Percent>{percent}</cbc:Percent>\n"));
        xml.push_str("        <cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme>\n");
        xml.push_str("      </cac:TaxCategory>\n");
        xml.push_str("    </cac:TaxSubtotal>\n");
    }
    xml.push_str("  </cac:TaxTotal>\n");
    xml.push_str("  <cac:LegalMonetaryTotal>\n");
    xml.push_str(&format!(
        "    <cbc:LineExtensionAmount currencyID=\"{currency}\">{}</cbc:LineExtensionAmount>\n",
        money(inv.net_cents)
    ));
    xml.push_str(&format!(
        "    <cbc:TaxExclusiveAmount currencyID=\"{currency}\">{}</cbc:TaxExclusiveAmount>\n",
        money(inv.net_cents)
    ));
    xml.push_str(&format!(
        "    <cbc:TaxInclusiveAmount currencyID=\"{currency}\">{}</cbc:TaxInclusiveAmount>\n",
        money(inv.gross_cents)
    ));
    xml.push_str(&format!(
        "    <cbc:PayableAmount currencyID=\"{currency}\">{}</cbc:PayableAmount>\n",
        money(inv.gross_cents)
    ));
    xml.push_str("  </cac:LegalMonetaryTotal>\n");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invoice_creation_and_recalculation() {
        let store = BillStore::open_in_memory().unwrap();
        let inv = store
            .create_invoice(
                "INV-001",
                "cust_acme",
                "ACME Corp GmbH",
                "finance@acme.at",
                Some("ATU12345678"),
                Some("AT89370400440532013000"),
                Some("RLBWWATW"),
                NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 11, 15).unwrap(),
                Some("Standard payment terms 14 days"),
            )
            .unwrap();

        assert_eq!(inv.status, InvoiceStatus::Draft);
        assert_eq!(inv.gross_cents, 0);

        store
            .add_item(
                "INV-001",
                "Architecture Consulting (hrs)",
                10.0,
                15000,
                0.20,
            )
            .unwrap();
        let fetched = store.get_invoice("INV-001").unwrap();
        assert_eq!(fetched.net_cents, 150000); // 1,500.00 EUR
        assert_eq!(fetched.tax_cents, 30000); // 300.00 EUR (20% USt)
        assert_eq!(fetched.gross_cents, 180000); // 1,800.00 EUR

        let eb = store.export_ebinterface("INV-001").unwrap();
        assert!(eb.contains("eb:InvoiceNumber=\"INV-001\""));
        assert!(eb.contains("<eb:VATIdentificationNumber>ATU12345678</eb:VATIdentificationNumber>"));
        assert!(eb.contains("<eb:TotalGrossAmount>1800.00</eb:TotalGrossAmount>"));

        let sepa = store
            .export_sepa_direct_debit(
                &["INV-001".to_string()],
                "Sandra Schipal Fleet",
                "AT021200000123456789",
                "BAWAGATW",
                "AT99ZZZ00000000001",
            )
            .unwrap();
        assert!(sepa.contains("<MsgId>SEPA-DD-"));
        assert!(sepa.contains("<InstdAmt Ccy=\"EUR\">1800.00</InstdAmt>"));

        store
            .save_xrechnung_party(
                "seller",
                &XrParty {
                    name: "BillCraft Test Seller".into(),
                    street: "Testgasse 1".into(),
                    city: "Wien".into(),
                    postcode: "1010".into(),
                    country: "DE".into(),
                    email: "seller@email.de".into(),
                    vat_id: Some("DE123456789".into()),
                    phone: Some("+49 1234-5678".into()),
                    iban: Some("DE79000000001234567890".into()),
                    buyer_reference: None,
                },
            )
            .unwrap();
        store
            .save_xrechnung_party(
                "INV-001",
                &XrParty {
                    name: "ACME Corp GmbH".into(),
                    street: "Kaeufergasse 2".into(),
                    city: "Wien".into(),
                    postcode: "1020".into(),
                    country: "DE".into(),
                    email: "buyer@info.de".into(),
                    vat_id: None,
                    phone: None,
                    iban: None,
                    buyer_reference: Some("04011000-12345-03".into()),
                },
            )
            .unwrap();
        let xr = store.export_xrechnung("INV-001").unwrap();
        assert!(xr.contains("xrechnung_3.0"));
        assert!(xr.contains("<cbc:ID>INV-001</cbc:ID>"));
        assert!(xr.contains("<cbc:PayableAmount currencyID=\"EUR\">1800.00</cbc:PayableAmount>"));
        assert!(xr.contains("Architecture Consulting (hrs)"));
        let dir = std::env::temp_dir().join(format!("billcraft-xr-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let xml_path = dir.join("INV-001.xml");
        std::fs::write(&xml_path, &xr).unwrap();
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/xrechnung-validator");
        let canon = std::fs::canonicalize(&root).unwrap();
        let canon_text = canon.to_string_lossy().to_string();
        let root = std::path::PathBuf::from(
            canon_text
                .strip_prefix(r"\\?\")
                .unwrap_or(canon_text.as_str()),
        );
        let jar = root.join("validator-1.6.3-standalone.jar");
        let scenarios = root.join("config").join("scenarios.xml");
        let repo = root.join("config");
        assert!(
            jar.is_file(),
            "KoSIT validator jar missing at {}",
            jar.display()
        );
        let output = std::process::Command::new("java")
            .arg("-jar")
            .arg(&jar)
            .arg("-s")
            .arg(&scenarios)
            .arg("-r")
            .arg(&repo)
            .arg("-o")
            .arg(&dir)
            .arg("-p")
            .arg(&xml_path)
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.status.success() && text.contains("Validation successful"),
            "{text}"
        );
    }
}
