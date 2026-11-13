#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

use billcraft_core::*;
use chrono::{Days, Local, NaiveDate};
use clap::Parser;
use craft_biz::bus::Bus;
use craft_biz::Money;
use craft_biz_ui::bookend::{self, Bar, Spec};
use craft_biz_ui::menu::Item;
use craft_biz_ui::theme::{self, tokens};
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

fn resolve_bus() -> Option<Bus> {
    if let Ok(dir) = std::env::var("CRAFT_BUS_DIR") {
        return Some(Bus::new(PathBuf::from(dir)));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return Some(Bus::new(PathBuf::from(local).join("craft-biz-bus")));
    }
    None
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
    let store = BillStore::open(resolve_db(args.db))?;
    let app = BillApp::new(BillService::new(store, resolve_bus()));
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 780.0])
            .with_title("BillCraft - Invoices"),
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
    service: BillService,
    invoices: Vec<Invoice>,
    subs: Vec<Subscription>,
    stats: BillStats,
    selected_invoice: Option<String>,
    items: Vec<InvoiceItem>,
    preview_title: String,
    preview: Option<String>,
    notice: String,
    bar: Bar,
    dark: bool,
    zoom: f32,
    draft_number: String,
    draft_name: String,
    draft_email: String,
    draft_iban: String,
    draft_bic: String,
    line_desc: String,
    line_qty: String,
    line_price: String,
    line_tax: String,
    seller_name: String,
    seller_street: String,
    seller_city: String,
    seller_postcode: String,
    seller_country: String,
    seller_email: String,
    seller_vat: String,
    seller_phone: String,
    seller_iban: String,
    seller_scheme: String,
    seller_peppol: String,
    buyer_street: String,
    buyer_city: String,
    buyer_postcode: String,
    buyer_country: String,
    buyer_ref: String,
    buyer_scheme: String,
    buyer_peppol: String,
    sepa_name: String,
    sepa_iban: String,
    sepa_bic: String,
    sepa_id: String,
    sub_id: String,
    sub_customer: String,
    sub_plan: String,
    sub_months: String,
    sub_price: String,
}

impl BillApp {
    fn new(service: BillService) -> Self {
        let seller = service
            .store()
            .get_xrechnung_party("seller")
            .ok()
            .flatten()
            .unwrap_or_else(empty_party);
        let mut app = Self {
            service,
            invoices: Vec::new(),
            subs: Vec::new(),
            stats: empty_stats(),
            selected_invoice: None,
            items: Vec::new(),
            preview_title: String::new(),
            preview: None,
            notice: String::new(),
            bar: Bar::new(),
            dark: true,
            zoom: 1.0,
            draft_number: String::new(),
            draft_name: String::new(),
            draft_email: String::new(),
            draft_iban: String::new(),
            draft_bic: String::new(),
            line_desc: String::new(),
            line_qty: "1".into(),
            line_price: String::new(),
            line_tax: "20".into(),
            seller_name: seller.name,
            seller_street: seller.street,
            seller_city: seller.city,
            seller_postcode: seller.postcode,
            seller_country: if seller.country.is_empty() {
                "AT".into()
            } else {
                seller.country
            },
            seller_email: seller.email,
            seller_vat: seller.vat_id.unwrap_or_default(),
            seller_phone: seller.phone.unwrap_or_default(),
            seller_iban: seller.iban.unwrap_or_default(),
            seller_scheme: seller.peppol_scheme.unwrap_or_default(),
            seller_peppol: seller.peppol_id.unwrap_or_default(),
            buyer_street: String::new(),
            buyer_city: String::new(),
            buyer_postcode: String::new(),
            buyer_country: "AT".into(),
            buyer_ref: String::new(),
            buyer_scheme: String::new(),
            buyer_peppol: String::new(),
            sepa_name: "Northwind Traders".into(),
            sepa_iban: "AT611904300234573201".into(),
            sepa_bic: "BKAUATWW".into(),
            sepa_id: "AT99ZZZ00000000001".into(),
            sub_id: String::new(),
            sub_customer: String::new(),
            sub_plan: String::new(),
            sub_months: "1".into(),
            sub_price: String::new(),
        };
        app.refresh();
        app
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
            file: vec![
                Item::action("Load Northwind demo", "app.demo"),
                Item::action("Drain inbox", "app.drain"),
                Item::action("Refresh", "app.refresh"),
            ],
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
        match change.action.as_deref() {
            Some("app.refresh") => {
                self.refresh();
                self.notice = "Refreshed.".into();
            }
            Some("app.demo") => self.load_demo(),
            Some("app.drain") => self.drain_inbox(),
            _ => {}
        }
    }

    fn refresh(&mut self) {
        let store = self.service.store();
        self.invoices = store.list_invoices(None).unwrap_or_default();
        self.subs = store.list_subscriptions().unwrap_or_default();
        self.stats = store.stats().unwrap_or_else(|_| empty_stats());
        if let Some(id) = &self.selected_invoice {
            self.items = store.get_items(id).unwrap_or_default();
        } else {
            self.items.clear();
        }
    }

    fn load_seller(&mut self) {
        if let Ok(Some(p)) = self.service.store().get_xrechnung_party("seller") {
            self.seller_name = p.name;
            self.seller_street = p.street;
            self.seller_city = p.city;
            self.seller_postcode = p.postcode;
            self.seller_country = p.country;
            self.seller_email = p.email;
            self.seller_vat = p.vat_id.unwrap_or_default();
            self.seller_phone = p.phone.unwrap_or_default();
            self.seller_iban = p.iban.unwrap_or_default();
            self.seller_scheme = p.peppol_scheme.unwrap_or_default();
            self.seller_peppol = p.peppol_id.unwrap_or_default();
        }
    }

    fn select(&mut self, id: String) {
        match self.service.store().get_xrechnung_party(&id) {
            Ok(Some(p)) => {
                self.buyer_street = p.street;
                self.buyer_city = p.city;
                self.buyer_postcode = p.postcode;
                self.buyer_country = p.country;
                self.buyer_ref = p.buyer_reference.unwrap_or_default();
                self.buyer_scheme = p.peppol_scheme.unwrap_or_default();
                self.buyer_peppol = p.peppol_id.unwrap_or_default();
            }
            _ => {
                self.buyer_street.clear();
                self.buyer_city.clear();
                self.buyer_postcode.clear();
                self.buyer_country = "AT".into();
                self.buyer_ref.clear();
                self.buyer_scheme.clear();
                self.buyer_peppol.clear();
            }
        }
        self.selected_invoice = Some(id);
        self.preview = None;
        self.preview_title.clear();
        self.refresh();
    }

    fn load_demo(&mut self) {
        match self.service.store().seed_northwind_demo() {
            Ok(n) => {
                self.notice =
                    format!("Loaded {n} Northwind Traders demo invoices. Each note says DEMO.");
                self.load_seller();
                let first = self
                    .service
                    .store()
                    .list_invoices(None)
                    .ok()
                    .and_then(|mut v| {
                        v.sort_by(|a, b| a.id.cmp(&b.id));
                        v.into_iter().next()
                    });
                if let Some(inv) = first {
                    self.select(inv.id);
                } else {
                    self.refresh();
                }
            }
            Err(e) => self.notice = e.to_string(),
        }
    }

    fn drain_inbox(&mut self) {
        match self.service.drain_inbox() {
            Ok(s) => {
                self.notice = format!(
                    "Inbox: {} events, {} draft invoices.",
                    s.processed,
                    s.created_invoices.len()
                );
                self.refresh();
            }
            Err(e) => self.notice = e.to_string(),
        }
    }

    fn create_invoice(&mut self) {
        let id = self.draft_number.trim().to_string();
        let name = self.draft_name.trim().to_string();
        let email = self.draft_email.trim().to_string();
        if id.is_empty() || name.is_empty() || email.is_empty() {
            self.notice = "Invoice number, customer name, and email are required.".into();
            return;
        }
        let today = Local::now().date_naive();
        let due = today.checked_add_days(Days::new(14)).unwrap_or(today);
        let iban = self.draft_iban.clone();
        let bic = self.draft_bic.clone();
        match self.service.store().create_invoice(
            &id,
            &customer_code(&name),
            &name,
            &email,
            None,
            blank(&iban),
            blank(&bic),
            today,
            due,
            None,
        ) {
            Ok(_) => {
                self.notice = format!("Invoice {id} created. Add a line on the right.");
                self.draft_number.clear();
                self.draft_name.clear();
                self.draft_email.clear();
                self.draft_iban.clear();
                self.draft_bic.clear();
                self.select(id);
            }
            Err(e) => self.notice = e.to_string(),
        }
    }

    fn add_line(&mut self) {
        let Some(id) = self.selected_invoice.clone() else {
            self.notice = "Create or select an invoice first.".into();
            return;
        };
        let desc = self.line_desc.trim().to_string();
        if desc.is_empty() {
            self.notice = "A line needs a description.".into();
            return;
        }
        let qty = match parse_qty(&self.line_qty) {
            Ok(q) => q,
            Err(e) => {
                self.notice = e;
                return;
            }
        };
        let cents = match Money::parse_de(&self.line_price) {
            Ok(m) if m.cents() >= 0 => m.cents(),
            Ok(_) => {
                self.notice = "Unit price must be zero or more. Use 150,00.".into();
                return;
            }
            Err(e) => {
                self.notice = format!("Unit price: {e}. Use 150,00.");
                return;
            }
        };
        let tax = match percent_to_rate(&self.line_tax) {
            Ok(r) => r,
            Err(e) => {
                self.notice = e;
                return;
            }
        };
        match self.service.store().add_item(&id, &desc, qty, cents, tax) {
            Ok(_) => {
                self.notice = format!("Line added to {id}.");
                self.line_desc.clear();
                self.refresh();
            }
            Err(e) => self.notice = e.to_string(),
        }
    }

    fn create_subscription(&mut self) {
        let id = self.sub_id.trim().to_string();
        let customer = self.sub_customer.trim().to_string();
        let plan = self.sub_plan.trim().to_string();
        if id.is_empty() || customer.is_empty() || plan.is_empty() {
            self.notice = "Subscription id, customer id, and plan are required.".into();
            return;
        }
        let months: i32 = match self.sub_months.trim().parse() {
            Ok(m) if m > 0 => m,
            _ => {
                self.notice = "Interval months must be a positive whole number.".into();
                return;
            }
        };
        let cents = match Money::parse_de(&self.sub_price) {
            Ok(m) if m.cents() > 0 => m.cents(),
            _ => {
                self.notice = "Subscription amount must be a positive amount such as 49,00.".into();
                return;
            }
        };
        let today = Local::now().date_naive();
        match self
            .service
            .store()
            .create_subscription(&id, &customer, &plan, months, cents, today)
        {
            Ok(_) => {
                self.notice = format!("Subscription {id} created.");
                self.sub_id.clear();
                self.sub_plan.clear();
                self.refresh();
            }
            Err(e) => self.notice = e.to_string(),
        }
    }

    fn show_export(&mut self, title: &str, result: Result<String, BillStoreError>) {
        match result {
            Ok(xml) => {
                self.preview_title = title.to_string();
                self.preview = Some(xml);
                self.notice = format!("{title} is in the panel below. Nothing was sent.");
            }
            Err(e) => {
                self.preview = None;
                self.notice = e.to_string();
            }
        }
    }

    fn save_seller(&mut self) -> Result<(), String> {
        if self.seller_name.trim().is_empty() {
            return Err("Your company name is required for XRechnung.".into());
        }
        let party = XrParty {
            name: self.seller_name.trim().to_string(),
            street: self.seller_street.trim().to_string(),
            city: self.seller_city.trim().to_string(),
            postcode: self.seller_postcode.trim().to_string(),
            country: self.seller_country.trim().to_string(),
            email: self.seller_email.trim().to_string(),
            vat_id: owned_blank(&self.seller_vat),
            phone: owned_blank(&self.seller_phone),
            iban: owned_blank(&self.seller_iban),
            buyer_reference: None,
            peppol_scheme: owned_blank(&self.seller_scheme),
            peppol_id: owned_blank(&self.seller_peppol),
        };
        self.service
            .store()
            .save_xrechnung_party("seller", &party)
            .map_err(|e| e.to_string())
    }

    fn save_buyer(&mut self, id: &str) -> Result<(), String> {
        let inv = self
            .service
            .store()
            .get_invoice(id)
            .map_err(|e| e.to_string())?;
        if self.buyer_street.trim().is_empty() || self.buyer_ref.trim().is_empty() {
            return Err("XRechnung needs the buyer street and a buyer reference.".into());
        }
        let party = XrParty {
            name: inv.customer_name,
            street: self.buyer_street.trim().to_string(),
            city: self.buyer_city.trim().to_string(),
            postcode: self.buyer_postcode.trim().to_string(),
            country: self.buyer_country.trim().to_string(),
            email: inv.customer_email,
            vat_id: inv.customer_uid,
            phone: None,
            iban: inv.customer_iban,
            buyer_reference: Some(self.buyer_ref.trim().to_string()),
            peppol_scheme: owned_blank(&self.buyer_scheme),
            peppol_id: owned_blank(&self.buyer_peppol),
        };
        self.service
            .store()
            .save_xrechnung_party(id, &party)
            .map_err(|e| e.to_string())
    }

    fn send_peppol(&mut self) {
        let Some(id) = self.selected_invoice.clone() else {
            self.notice = "Select an invoice first.".into();
            return;
        };
        if let Err(e) = self.save_seller() {
            self.notice = e;
            return;
        }
        if let Err(e) = self.save_buyer(&id) {
            self.notice = e;
            return;
        }
        match self.service.peppol_send(&id) {
            Ok(out) => {
                self.preview_title = format!("Peppol {}", out.status);
                self.preview = Some(out.package);
                self.notice = format!("{} {}. {}", out.status, out.message_id, out.detail);
            }
            Err(e) => {
                self.preview = None;
                self.notice = e.to_string();
            }
        }
    }

    fn export_xrechnung(&mut self) {
        let Some(id) = self.selected_invoice.clone() else {
            self.notice = "Select an invoice first.".into();
            return;
        };
        if let Err(e) = self.save_seller() {
            self.notice = e;
            return;
        }
        if let Err(e) = self.save_buyer(&id) {
            self.notice = e;
            return;
        }
        let xml = self.service.store().export_xrechnung(&id);
        self.show_export("XRechnung", xml);
    }

    fn export_ebinterface(&mut self) {
        let Some(id) = self.selected_invoice.clone() else {
            self.notice = "Select an invoice first.".into();
            return;
        };
        let xml = self.service.store().export_ebinterface(&id);
        self.show_export("ebInterface", xml);
    }

    fn export_sepa(&mut self) {
        let Some(id) = self.selected_invoice.clone() else {
            self.notice = "Select an invoice first.".into();
            return;
        };
        let name = self.sepa_name.clone();
        let iban = self.sepa_iban.clone();
        let bic = self.sepa_bic.clone();
        let creditor = self.sepa_id.clone();
        let xml =
            self.service
                .store()
                .export_sepa_direct_debit(&[id], &name, &iban, &bic, &creditor);
        self.show_export("SEPA direct debit", xml);
    }

    fn paint_create(&mut self, ui: &mut egui::Ui) {
        ui.heading("New invoice");
        ui.horizontal(|ui| {
            labeled(ui, "Number", &mut self.draft_number, 110.0);
            labeled(ui, "Customer", &mut self.draft_name, 160.0);
            labeled(ui, "Email", &mut self.draft_email, 180.0);
        });
        ui.horizontal(|ui| {
            labeled(ui, "IBAN", &mut self.draft_iban, 220.0);
            labeled(ui, "BIC", &mut self.draft_bic, 110.0);
            if ui.button("Create invoice").clicked() {
                self.create_invoice();
            }
            if ui.button("Load Northwind demo").clicked() {
                self.load_demo();
            }
        });
    }

    fn paint_list(&mut self, ui: &mut egui::Ui) {
        ui.heading("Invoices");
        if self.invoices.is_empty() {
            ui.label("No invoices yet. Create one, or load the Northwind Traders demo.");
        }
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .show(ui, |ui| {
                egui::Grid::new("invoice_grid")
                    .striped(true)
                    .min_col_width(80.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Number").strong());
                        ui.label(RichText::new("Customer").strong());
                        ui.label(RichText::new("Status").strong());
                        ui.label(RichText::new("Due").strong());
                        ui.label(RichText::new("Gross").strong());
                        ui.label("");
                        ui.end_row();
                        let mut select_inv = None;
                        for inv in &self.invoices {
                            ui.label(&inv.id);
                            ui.label(&inv.customer_name);
                            ui.label(inv.status.as_str());
                            ui.label(inv.due_date.to_string());
                            ui.label(Money::from_cents(inv.gross_cents).format_eur());
                            if ui.button("Open").clicked() {
                                select_inv = Some(inv.id.clone());
                            }
                            ui.end_row();
                        }
                        if let Some(id) = select_inv {
                            self.select(id);
                        }
                    });
            });
    }

    fn paint_detail(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.selected_invoice.clone() else {
            ui.heading("Invoice");
            ui.label("Create an invoice on the left. Its lines and exports show up here.");
            return;
        };
        let Ok(inv) = self.service.store().get_invoice(&id) else {
            ui.label("That invoice is no longer in the file.");
            return;
        };
        ui.heading(format!("Invoice {id}"));
        ui.label(format!("{} <{}>", inv.customer_name, inv.customer_email));
        ui.label(format!(
            "{}  due {}  {}",
            inv.status.as_str(),
            inv.due_date,
            Money::from_cents(inv.gross_cents).format_eur()
        ));
        ui.label(format!(
            "Net {}   USt {}",
            Money::from_cents(inv.net_cents).format_eur(),
            Money::from_cents(inv.tax_cents).format_eur()
        ));
        if let Some(note) = &inv.notes {
            ui.label(note);
        }
        ui.separator();
        ui.label(RichText::new("Lines").strong());
        if self.items.is_empty() {
            ui.label("No lines yet.");
        }
        for item in &self.items {
            ui.label(format!(
                "{}. {}   {} x {}   {}",
                item.position,
                item.description,
                trim_qty(item.quantity),
                Money::from_cents(item.unit_price_cents).format_eur(),
                Money::from_cents(item.total_cents).format_eur()
            ));
        }
        ui.horizontal(|ui| {
            labeled(ui, "Description", &mut self.line_desc, 130.0);
            labeled(ui, "Qty", &mut self.line_qty, 44.0);
            labeled(ui, "Price", &mut self.line_price, 72.0);
            labeled(ui, "USt %", &mut self.line_tax, 36.0);
            if ui.button("Add line").clicked() {
                self.add_line();
            }
        });
        ui.label("Price is Austrian: 150,00 or 1.250,50.");
        ui.horizontal(|ui| {
            if ui.button("Issue").clicked() {
                self.notice = match self.service.issue_invoice(&id) {
                    Ok(_) => format!("{id} issued."),
                    Err(e) => e.to_string(),
                };
                self.refresh();
            }
            if ui.button("Mark paid").clicked() {
                self.notice = match self.service.mark_paid(&id) {
                    Ok(_) => format!("{id} marked paid."),
                    Err(e) => e.to_string(),
                };
                self.refresh();
            }
            if ui.button("Flag overdue").clicked() {
                self.notice = match self.service.flag_overdue(&id) {
                    Ok(_) => format!("{id} flagged overdue."),
                    Err(e) => e.to_string(),
                };
                self.refresh();
            }
        });
        ui.separator();
        ui.label(RichText::new("Buyer address for XRechnung").strong());
        ui.horizontal(|ui| {
            labeled(ui, "Street", &mut self.buyer_street, 140.0);
            labeled(ui, "City", &mut self.buyer_city, 100.0);
        });
        ui.horizontal(|ui| {
            labeled(ui, "Postcode", &mut self.buyer_postcode, 70.0);
            labeled(ui, "Country", &mut self.buyer_country, 40.0);
            labeled(ui, "Buyer ref", &mut self.buyer_ref, 120.0);
        });
        ui.horizontal(|ui| {
            labeled(ui, "Scheme", &mut self.buyer_scheme, 50.0);
            labeled(ui, "Peppol id", &mut self.buyer_peppol, 160.0);
        });
        ui.horizontal(|ui| {
            if ui.button("ebInterface").clicked() {
                self.export_ebinterface();
            }
            if ui.button("XRechnung").clicked() {
                self.export_xrechnung();
            }
            if ui.button("SEPA debit").clicked() {
                self.export_sepa();
            }
            if ui.button("Send on Peppol").clicked() {
                self.send_peppol();
            }
        });
        ui.horizontal(|ui| {
            labeled(ui, "Creditor", &mut self.sepa_name, 120.0);
            labeled(ui, "IBAN", &mut self.sepa_iban, 160.0);
        });
        ui.horizontal(|ui| {
            labeled(ui, "BIC", &mut self.sepa_bic, 90.0);
            labeled(ui, "Creditor id", &mut self.sepa_id, 150.0);
        });
    }

    fn paint_company(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Your company (saved for XRechnung)")
            .default_open(self.seller_name.is_empty())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    labeled(ui, "Name", &mut self.seller_name, 150.0);
                    labeled(ui, "Email", &mut self.seller_email, 150.0);
                });
                ui.horizontal(|ui| {
                    labeled(ui, "Street", &mut self.seller_street, 150.0);
                    labeled(ui, "City", &mut self.seller_city, 100.0);
                });
                ui.horizontal(|ui| {
                    labeled(ui, "Postcode", &mut self.seller_postcode, 70.0);
                    labeled(ui, "Country", &mut self.seller_country, 40.0);
                });
                ui.horizontal(|ui| {
                    labeled(ui, "VAT id", &mut self.seller_vat, 110.0);
                    labeled(ui, "Phone", &mut self.seller_phone, 110.0);
                });
                labeled(ui, "IBAN", &mut self.seller_iban, 220.0);
                ui.horizontal(|ui| {
                    labeled(ui, "Scheme", &mut self.seller_scheme, 50.0);
                    labeled(ui, "Peppol id", &mut self.seller_peppol, 160.0);
                });
                ui.label("Scheme is the ISO 6523 code, such as 9930. The id is the participant, not the email.");
                if ui.button("Save company").clicked() {
                    self.notice = match self.save_seller() {
                        Ok(()) => "Company saved.".into(),
                        Err(e) => e,
                    };
                }
            });
    }

    fn paint_subscriptions(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Subscriptions").show(ui, |ui| {
            for s in &self.subs {
                ui.label(format!(
                    "{}  {}  {}  every {} mo  {}",
                    s.id,
                    s.customer_id,
                    s.plan_name,
                    s.interval_months,
                    Money::from_cents(s.amount_cents).format_eur()
                ));
            }
            ui.horizontal(|ui| {
                labeled(ui, "Id", &mut self.sub_id, 80.0);
                labeled(ui, "Customer", &mut self.sub_customer, 80.0);
                labeled(ui, "Plan", &mut self.sub_plan, 80.0);
            });
            ui.horizontal(|ui| {
                labeled(ui, "Months", &mut self.sub_months, 40.0);
                labeled(ui, "Amount", &mut self.sub_price, 70.0);
                if ui.button("Create subscription").clicked() {
                    self.create_subscription();
                }
            });
        });
    }
}

impl eframe::App for BillApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        theme::apply(ui.ctx(), self.dark);
        self.paint_menu(ui);
        let accent = tokens(self.dark).accent;
        ui.horizontal(|ui| {
            ui.heading(RichText::new("BillCraft").strong().color(accent));
            ui.separator();
            ui.label(
                "Create an invoice, add lines, then export. Nothing is sent to a bank or Peppol.",
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.dark {
                    "Light theme"
                } else {
                    "Dark theme"
                };
                if ui.button(label).clicked() {
                    self.dark = !self.dark;
                    theme::apply(ui.ctx(), self.dark);
                }
            });
        });
        if !self.notice.is_empty() {
            ui.label(RichText::new(&self.notice).color(accent));
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            kpi(
                ui,
                "Invoiced",
                self.stats.total_gross_cents,
                tokens(self.dark).text,
            );
            ui.separator();
            kpi(
                ui,
                "Paid",
                self.stats.paid_gross_cents,
                Color32::from_rgb(46, 204, 113),
            );
            ui.separator();
            kpi(
                ui,
                "Overdue",
                self.stats.overdue_gross_cents,
                Color32::from_rgb(231, 76, 60),
            );
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_min_width(540.0);
                self.paint_create(ui);
                ui.add_space(6.0);
                self.paint_list(ui);
                self.paint_subscriptions(ui);
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_min_width(480.0);
                self.paint_detail(ui);
                ui.add_space(6.0);
                self.paint_company(ui);
                if let Some(xml) = &self.preview {
                    ui.add_space(6.0);
                    ui.label(RichText::new(&self.preview_title).strong());
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .show(ui, |ui| {
                            ui.code(xml);
                        });
                }
            });
        });
    }
}

fn empty_party() -> XrParty {
    XrParty {
        name: String::new(),
        street: String::new(),
        city: String::new(),
        postcode: String::new(),
        country: "AT".into(),
        email: String::new(),
        vat_id: None,
        phone: None,
        iban: None,
        buyer_reference: None,
        peppol_scheme: None,
        peppol_id: None,
    }
}

fn empty_stats() -> BillStats {
    BillStats {
        total_invoices: 0,
        total_net_cents: 0,
        total_tax_cents: 0,
        total_gross_cents: 0,
        paid_gross_cents: 0,
        overdue_gross_cents: 0,
        mrr_cents: 0,
    }
}

fn kpi(ui: &mut egui::Ui, name: &str, cents: i64, color: Color32) {
    ui.label(
        RichText::new(format!("{name}: {}", Money::from_cents(cents).format_eur()))
            .color(color)
            .strong(),
    );
}

fn labeled(ui: &mut egui::Ui, name: &str, value: &mut String, width: f32) {
    ui.label(name);
    ui.add(egui::TextEdit::singleline(value).desired_width(width));
}

fn blank(value: &str) -> Option<&str> {
    let t = value.trim();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

fn owned_blank(value: &str) -> Option<String> {
    blank(value).map(str::to_string)
}

fn customer_code(name: &str) -> String {
    let code: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(24)
        .collect();
    if code.is_empty() {
        "customer".into()
    } else {
        code
    }
}

fn parse_qty(raw: &str) -> Result<f64, String> {
    let q: f64 = raw
        .trim()
        .replace(',', ".")
        .parse()
        .map_err(|_| "quantity is not a number".to_string())?;
    if !q.is_finite() || q <= 0.0 {
        return Err("quantity must be greater than zero".into());
    }
    Ok(q)
}

fn percent_to_rate(raw: &str) -> Result<f64, String> {
    let p: f64 = raw
        .trim()
        .replace(',', ".")
        .parse()
        .map_err(|_| "tax percent is not a number".to_string())?;
    if !(0.0..=100.0).contains(&p) {
        return Err("tax percent must be between 0 and 100".into());
    }
    Ok(p / 100.0)
}

fn trim_qty(q: f64) -> String {
    if (q - q.round()).abs() < 1e-9 {
        format!("{}", q.round() as i64)
    } else {
        format!("{q}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tax_percent_and_customer_code() -> Result<(), String> {
        if (percent_to_rate("20")? - 0.2).abs() > 1e-9 {
            return Err("20 percent".into());
        }
        if percent_to_rate("101").is_ok() {
            return Err("101 should fail".into());
        }
        if (parse_qty("1,5")? - 1.5).abs() > 1e-9 {
            return Err("quantity".into());
        }
        if customer_code("ACME GmbH") != "ACMEGmbH" || customer_code("...") != "customer" {
            return Err("customer code".into());
        }
        Ok(())
    }
}
