#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

use billcraft_core::*;
use chrono::NaiveDate;
use clap::Parser;
use craft_biz_ui::bookend::{self, Bar, Spec};
use craft_biz_ui::menu::Item;
use eframe::egui::{self, Color32, RichText};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "billcraft-app",
    about = "Native egui desktop host for BillCraft"
)]
struct Args {
    #[arg(long)]
    smoke: bool,
    #[arg(long)]
    db: Option<PathBuf>,
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

fn run_smoke_checks() -> Result<(), Box<dyn std::error::Error>> {
    let store = BillStore::open_in_memory()?;
    let inv = store.create_invoice(
        "SMOKE-001",
        "cust_test",
        "Test GmbH",
        "test@test.at",
        Some("ATU99999999"),
        Some("AT000000000000000000"),
        Some("TESTBIC"),
        NaiveDate::from_ymd_opt(2026, 11, 1).unwrap_or_default(),
        NaiveDate::from_ymd_opt(2026, 11, 15).unwrap_or_default(),
        Some("Smoke invoice"),
    )?;
    store.add_item(&inv.id, "Cloud Maintenance", 5.0, 10000, 0.20)?;
    let eb = store.export_ebinterface(&inv.id)?;
    if !eb.contains("SMOKE-001") {
        return Err("ebInterface export failed smoke verification".into());
    }
    bookend::smoke("BillCraft", env!("CARGO_PKG_VERSION"), "invoices")?;
    println!("smoke: ok (billcraft-app)");
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.smoke {
        return run_smoke_checks();
    }

    let db_path = resolve_db(args.db);
    let store = BillStore::open(&db_path)?;
    let app = BillApp::new(store);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1040.0, 680.0])
            .with_title("BillCraft — Sovereign e-Invoicing & SEPA Engine"),
        ..Default::default()
    };

    eframe::run_native(
        "BillCraft",
        native_options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
    .map_err(|e| format!("Eframe runtime error: {e}").into())
}

struct BillApp {
    store: BillStore,
    invoices: Vec<Invoice>,
    stats: BillStats,
    selected_invoice: Option<String>,
    items: Vec<InvoiceItem>,
    eb_preview: Option<String>,
    bar: Bar,
    dark: bool,
    zoom: f32,
}

impl BillApp {
    fn new(store: BillStore) -> Self {
        let invoices = store.list_invoices(None).unwrap_or_default();
        let stats = store.stats().unwrap_or(BillStats {
            total_invoices: 0,
            total_net_cents: 0,
            total_tax_cents: 0,
            total_gross_cents: 0,
            paid_gross_cents: 0,
            overdue_gross_cents: 0,
            mrr_cents: 0,
        });
        Self {
            store,
            invoices,
            stats,
            selected_invoice: None,
            items: Vec::new(),
            eb_preview: None,
            bar: Bar::new(),
            dark: true,
            zoom: 1.0,
        }
    }

    fn paint_menu(&mut self, ui: &mut egui::Ui) {
        let docs = bookend::repo_docs(env!("CARGO_MANIFEST_DIR"));
        let spec = Spec {
            name: "BillCraft",
            version: env!("CARGO_PKG_VERSION"),
            description: "Invoices, ebInterface, pain.008, and XRechnung.",
            repository: "https://github.com/sandraschi/billcraft",
            docs: &docs,
            dark: self.dark,
            zoom: self.zoom,
            file: vec![Item::action("Refresh", "app.refresh")],
            view: Vec::new(),
            middle: Vec::new(),
        };
        let change = self.bar.show(ui, &spec);
        if let Some(dark) = change.dark {
            self.dark = dark;
        }
        if let Some(zoom) = change.zoom {
            self.zoom = zoom;
        }
        if change.action.as_deref() == Some("app.refresh") {
            self.refresh();
        }
    }

    fn refresh(&mut self) {
        if let Ok(invs) = self.store.list_invoices(None) {
            self.invoices = invs;
        }
        if let Ok(st) = self.store.stats() {
            self.stats = st;
        }
        if let Some(id) = &self.selected_invoice {
            if let Ok(its) = self.store.get_items(id) {
                self.items = its;
            }
        }
    }
}

impl eframe::App for BillApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.paint_menu(ui);
        ui.horizontal(|ui| {
            ui.heading(
                RichText::new("💶 BillCraft")
                    .strong()
                    .color(Color32::from_rgb(46, 204, 113)),
            );
            ui.separator();
            ui.label(
                RichText::new("Sovereign e-Invoicing (ebInterface 6.1) & SEPA ISO 20022 Engine")
                    .weak(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("🔄 Refresh").clicked() {
                    self.refresh();
                }
            });
        });
        ui.add_space(4.0);

        // Live KPI HUD
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "Invoiced: €{:.2}",
                    self.stats.total_gross_cents as f64 / 100.0
                ))
                .strong(),
            );
            ui.separator();
            ui.label(
                RichText::new(format!(
                    "Paid: €{:.2}",
                    self.stats.paid_gross_cents as f64 / 100.0
                ))
                .color(Color32::from_rgb(46, 204, 113)),
            );
            ui.separator();
            ui.label(
                RichText::new(format!(
                    "Overdue: €{:.2}",
                    self.stats.overdue_gross_cents as f64 / 100.0
                ))
                .color(Color32::from_rgb(231, 76, 60)),
            );
            ui.separator();
            ui.label(
                RichText::new(format!(
                    "Active MRR: €{:.2}",
                    self.stats.mrr_cents as f64 / 100.0
                ))
                .color(Color32::from_rgb(52, 152, 219)),
            );
        });
        ui.separator();

        ui.horizontal(|ui| {
            // Left main table
            ui.vertical(|ui| {
                ui.heading("Invoices Registry");
                egui::ScrollArea::vertical().max_width(620.0).show(ui, |ui| {
                    egui::Grid::new("invoice_grid").striped(true).min_col_width(90.0).show(ui, |ui| {
                        ui.label(RichText::new("ID").strong());
                        ui.label(RichText::new("Customer").strong());
                        ui.label(RichText::new("Status").strong());
                        ui.label(RichText::new("Due Date").strong());
                        ui.label(RichText::new("Gross Amount").strong());
                        ui.label(RichText::new("Action").strong());
                        ui.end_row();

                        let mut select_inv = None;
                        for inv in &self.invoices {
                            ui.label(&inv.id);
                            ui.label(&inv.customer_name);
                            let status_color = match inv.status {
                                InvoiceStatus::Paid => Color32::from_rgb(46, 204, 113),
                                InvoiceStatus::Overdue => Color32::from_rgb(231, 76, 60),
                                InvoiceStatus::Issued => Color32::from_rgb(52, 152, 219),
                                _ => Color32::GRAY,
                            };
                            ui.label(RichText::new(inv.status.as_str()).color(status_color));
                            ui.label(inv.due_date.to_string());
                            ui.label(format!("€{:.2}", inv.gross_cents as f64 / 100.0));
                            if ui.button("Inspect").clicked() {
                                select_inv = Some(inv.id.clone());
                            }
                            ui.end_row();
                        }
                        if let Some(id) = select_inv {
                            self.selected_invoice = Some(id);
                            self.refresh();
                        }
                    });
                });
            });

            ui.separator();

            // Right details inspector
            ui.vertical(|ui| {
                if let Some(id) = &self.selected_invoice.clone() {
                    ui.heading(format!("Invoice: {id}"));
                    ui.add_space(4.0);

                    if let Ok(inv) = self.store.get_invoice(id) {
                        ui.label(format!("Customer: {}", inv.customer_name));
                        ui.label(format!("Email: {}", inv.customer_email));
                        ui.label(format!("Status: {:?}", inv.status));
                        ui.label(format!("Due: {}", inv.due_date));
                        ui.separator();
                        ui.label(RichText::new(format!("Gross Total: €{:.2}", inv.gross_cents as f64 / 100.0)).strong().size(18.0));
                        ui.label(format!("Net: €{:.2} | USt (20%): €{:.2}", inv.net_cents as f64 / 100.0, inv.tax_cents as f64 / 100.0));
                        ui.separator();

                        ui.horizontal(|ui| {
                            if ui.button("Mark Paid").clicked() {
                                let _ = self.store.update_status(id, InvoiceStatus::Paid);
                                self.refresh();
                            }
                            if ui.button("Flag Overdue").clicked() {
                                let _ = self.store.update_status(id, InvoiceStatus::Overdue);
                                self.refresh();
                            }
                        });

                        ui.add_space(8.0);
                        if ui.button("📄 Export ebInterface 6.1 XML").clicked() {
                            self.eb_preview = self.store.export_ebinterface(id).ok();
                        }

                        if let Some(xml) = &self.eb_preview {
                            ui.add_space(4.0);
                            ui.label(RichText::new("Austrian ebInterface 6.1 Output:").weak());
                            egui::ScrollArea::vertical().max_height(250.0).show(ui, |ui| {
                                ui.code(xml);
                            });
                        }
                    }
                } else {
                    ui.label(RichText::new("Select an invoice on the left to inspect line items or export e-invoicing XML.").weak());
                }
            });
        });
    }
}
