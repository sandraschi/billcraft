#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

use billcraft_core::*;
use chrono::Local;
use clap::{Parser, Subcommand};
use craft_biz::bus::Bus;
use serde_json::json;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "billcraft", author, version, about = "Local-first invoicing, ebInterface 6.1 & SEPA engine. CLI + MCP.")]
struct Cli {
    #[arg(long)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Status,
    CreateInvoice {
        id: String,
        customer_id: String,
        customer_name: String,
        customer_email: String,
        #[arg(long)]
        uid: Option<String>,
        #[arg(long)]
        iban: Option<String>,
        #[arg(long)]
        bic: Option<String>,
        #[arg(long)]
        days: Option<i64>,
    },
    AddItem {
        invoice_id: String,
        description: String,
        quantity: f64,
        unit_price_cents: i64,
        #[arg(long, default_value = "0.20")]
        tax_rate: f64,
    },
    Show {
        id: String,
    },
    List {
        #[arg(long)]
        status: Option<String>,
    },
    Issue {
        id: String,
    },
    MarkPaid {
        id: String,
    },
    FlagOverdue {
        id: String,
    },
    ExportEbinterface {
        id: String,
    },
    ExportSepa {
        invoice_ids: Vec<String>,
        #[arg(long, default_value = "Fleet Organization")]
        creditor_name: String,
        #[arg(long, default_value = "AT021200000123456789")]
        creditor_iban: String,
        #[arg(long, default_value = "BAWAGATW")]
        creditor_bic: String,
        #[arg(long, default_value = "AT99ZZZ00000000001")]
        creditor_id: String,
    },
    SubCreate {
        id: String,
        customer_id: String,
        plan_name: String,
        interval_months: i32,
        amount_cents: i64,
    },
    SubList,
    Stats,
    InboxDrain,
    Tools,
    Mcp,
}

fn resolve_db(path: Option<PathBuf>) -> PathBuf {
    if let Some(p) = path {
        return p;
    }
    if let Ok(p) = std::env::var("BILLCRAFT_DB") {
        return PathBuf::from(p);
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let dir = PathBuf::from(local).join("billcraft");
        let _ = std::fs::create_dir_all(&dir);
        return dir.join("billcraft.db");
    }
    PathBuf::from("billcraft.db")
}

fn resolve_bus() -> Option<Bus> {
    if let Ok(dir) = std::env::var("CRAFT_BUS_DIR") {
        return Some(Bus::new(PathBuf::from(dir)));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return Some(Bus::new(PathBuf::from(local).join("craft-biz-bus")));
    }
    None
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let db_path = resolve_db(cli.db);
    let store = BillStore::open(&db_path)?;
    let bus = resolve_bus();
    let service = BillService::new(store, bus);

    let Some(cmd) = cli.command else {
        println!("BillCraft — Local-first e-Invoicing & SEPA engine. Use --help for subcommands.");
        return Ok(());
    };

    match cmd {
        Commands::Status => {
            let stats = service.store().stats()?;
            println!("Invoices: {}", stats.total_invoices);
            println!("Gross Invoiced: €{:.2}", stats.total_gross_cents as f64 / 100.0);
            println!("Paid: €{:.2}", stats.paid_gross_cents as f64 / 100.0);
            println!("Overdue: €{:.2}", stats.overdue_gross_cents as f64 / 100.0);
            println!("MRR: €{:.2}", stats.mrr_cents as f64 / 100.0);
        }
        Commands::CreateInvoice { id, customer_id, customer_name, customer_email, uid, iban, bic, days } => {
            let today = Local::now().date_naive();
            let due = today + chrono::Days::new(days.unwrap_or(14) as u64);
            let inv = service.store().create_invoice(
                &id,
                &customer_id,
                &customer_name,
                &customer_email,
                uid.as_deref(),
                iban.as_deref(),
                bic.as_deref(),
                today,
                due,
                None,
            )?;
            println!("Created invoice {} for {} (Status: {:?})", inv.id, inv.customer_name, inv.status);
        }
        Commands::AddItem { invoice_id, description, quantity, unit_price_cents, tax_rate } => {
            let it = service.store().add_item(&invoice_id, &description, quantity, unit_price_cents, tax_rate)?;
            let inv = service.store().get_invoice(&invoice_id)?;
            println!("Added item {} to {}. Invoice gross now: €{:.2}", it.id, invoice_id, inv.gross_cents as f64 / 100.0);
        }
        Commands::Show { id } => {
            let inv = service.store().get_invoice(&id)?;
            let items = service.store().get_items(&id)?;
            println!("Invoice: {}", inv.id);
            println!("Customer: {} ({})", inv.customer_name, inv.customer_email);
            println!("Status: {:?}", inv.status);
            println!("Net: €{:.2} | USt (20%): €{:.2} | Gross: €{:.2}", inv.net_cents as f64 / 100.0, inv.tax_cents as f64 / 100.0, inv.gross_cents as f64 / 100.0);
            println!("Items ({}):", items.len());
            for it in items {
                println!("  #{}: {} - {:.1}x @ €{:.2} = €{:.2}", it.position, it.description, it.quantity, it.unit_price_cents as f64 / 100.0, it.total_cents as f64 / 100.0);
            }
        }
        Commands::List { status } => {
            let filter = status.as_deref().and_then(InvoiceStatus::from_str_opt);
            let invoices = service.store().list_invoices(filter)?;
            for i in invoices {
                println!("{:14} {:20} {:10} €{:.2}", i.id, i.customer_name, i.status.as_str(), i.gross_cents as f64 / 100.0);
            }
        }
        Commands::Issue { id } => {
            let inv = service.issue_invoice(&id)?;
            println!("Issued invoice {} (Gross: €{:.2})", inv.id, inv.gross_cents as f64 / 100.0);
        }
        Commands::MarkPaid { id } => {
            let inv = service.mark_paid(&id)?;
            println!("Marked invoice {} as PAID", inv.id);
        }
        Commands::FlagOverdue { id } => {
            let inv = service.flag_overdue(&id)?;
            println!("Marked invoice {} as OVERDUE and emitted penalty signal", inv.id);
        }
        Commands::ExportEbinterface { id } => {
            let xml = service.store().export_ebinterface(&id)?;
            println!("{xml}");
        }
        Commands::ExportSepa { invoice_ids, creditor_name, creditor_iban, creditor_bic, creditor_id } => {
            let xml = service.store().export_sepa_direct_debit(
                &invoice_ids,
                &creditor_name,
                &creditor_iban,
                &creditor_bic,
                &creditor_id,
            )?;
            println!("{xml}");
        }
        Commands::SubCreate { id, customer_id, plan_name, interval_months, amount_cents } => {
            let next = Local::now().date_naive() + chrono::Months::new(interval_months as u32);
            let sub = service.store().create_subscription(&id, &customer_id, &plan_name, interval_months, amount_cents, next)?;
            println!("Created subscription {} for {} at €{:.2}/{}mo", sub.id, sub.customer_id, sub.amount_cents as f64 / 100.0, sub.interval_months);
        }
        Commands::SubList => {
            let subs = service.store().list_subscriptions()?;
            for s in subs {
                println!("{:12} {:16} {:14} €{:.2} (next: {})", s.id, s.customer_id, s.plan_name, s.amount_cents as f64 / 100.0, s.next_billing_date);
            }
        }
        Commands::Stats => {
            let s = service.store().stats()?;
            println!("{}", serde_json::to_string_pretty(&s)?);
        }
        Commands::InboxDrain => {
            let res = service.drain_inbox()?;
            println!("Drained {} events from inbox. Invoices created: {:?}", res.processed, res.created_invoices);
        }
        Commands::Tools => {
            print_tools_catalog();
        }
        Commands::Mcp => {
            run_mcp_server(service)?;
        }
    }

    Ok(())
}

fn print_tools_catalog() {
    let catalog = json!([
        { "name": "bill_stats", "description": "Get overall billing statistics (invoiced, paid, overdue, MRR)" },
        { "name": "invoice_create", "description": "Create a new draft invoice" },
        { "name": "invoice_add_item", "description": "Add a line item to an invoice" },
        { "name": "invoice_get", "description": "Get invoice details and line items" },
        { "name": "invoice_list", "description": "List invoices filtered by status" },
        { "name": "invoice_issue", "description": "Issue an invoice and broadcast event" },
        { "name": "invoice_mark_paid", "description": "Mark an invoice as paid" },
        { "name": "invoice_flag_overdue", "description": "Flag invoice as overdue and emit penalty" },
        { "name": "ebinterface_export", "description": "Export Austrian ebInterface 6.1 compliant XML" },
        { "name": "sepa_export", "description": "Export ISO 20022 pain.008 SEPA direct debit XML" },
        { "name": "subscription_create", "description": "Create recurring subscription" },
        { "name": "subscription_list", "description": "List active customer subscriptions" },
        { "name": "inbox_drain", "description": "Drain cross-app events from teamcraft/contractcraft" }
    ]);
    println!("{}", serde_json::to_string_pretty(&catalog).unwrap_or_default());
}

fn run_mcp_server(service: BillService) -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let Ok(req) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };

        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let id = req.get("id").cloned();

        match method {
            "initialize" => {
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "serverInfo": { "name": "billcraft-mcp", "version": "0.1.0" },
                        "capabilities": { "tools": {} }
                    }
                });
                writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
                stdout.flush()?;
            }
            "tools/list" => {
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": [
                            { "name": "bill_stats", "description": "Get billing totals and MRR", "inputSchema": { "type": "object" } },
                            { "name": "invoice_list", "description": "List invoices by status", "inputSchema": { "type": "object" } },
                            { "name": "invoice_get", "description": "Get invoice details", "inputSchema": { "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] } }
                        ]
                    }
                });
                writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
                stdout.flush()?;
            }
            "tools/call" => {
                let tool_name = req.get("params").and_then(|p| p.get("name")).and_then(|n| n.as_str()).unwrap_or("");
                let result_json = match tool_name {
                    "bill_stats" => {
                        let st = service.store().stats().map_err(|e| e.to_string())?;
                        json!(st)
                    }
                    "invoice_list" => {
                        let invs = service.store().list_invoices(None).map_err(|e| e.to_string())?;
                        json!(invs)
                    }
                    _ => json!({ "status": "unknown tool" }),
                };
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&result_json)? }]
                    }
                });
                writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
                stdout.flush()?;
            }
            _ => {
                if let Some(id) = id {
                    let resp = json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "Method not found" } });
                    writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
                    stdout.flush()?;
                }
            }
        }
    }
    Ok(())
}
