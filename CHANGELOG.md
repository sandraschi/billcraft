# Changelog

All notable changes to `billcraft` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] - 2026-11-13

### Changed
- `export_xrechnung` writes the seller, buyer, payment account, tax breakdown, and lines required by XRechnung 3.0.2. The store test runs KoSIT Validator 1.6.3. The fixture was accepted. Missing party data is an error.
- The window has File, Edit, View, Window, and Help. Help opens the five standard dialogs. `--smoke` checks the menu.

## [0.1.0] - 2026-11-10

### Added
- Pure-Rust 4-crate workspace architecture (`billcraft-store`, `billcraft-core`, `billcraft-mcp`, `billcraft-app`).
- Austrian ebInterface 6.1 XML generator compliant with federal e-invoicing standards (`ebInterface:Invoice`).
- ISO 20022 SEPA Direct Debit (`pain.008.001.02`) XML batch payment order generator.
- Recurring subscription billing engine with monthly and annual billing periods.
- Event bus integration (`craft-biz::bus::Bus`):
  - Inbound: drains `contract.signed` from `contractcraft` and `timesheet.approved` from `teamcraft`.
  - Outbound: emits `invoice.issued`, `invoice.paid`, and `invoice.overdue`.
- Stdio FastMCP JSON-RPC server and CLI with 8 tools (`bill_status`, `bill_customer_add`, `bill_customer_list`, `bill_invoice_draft`, `bill_invoice_issue`, `bill_invoice_show`, `bill_ebinterface_export`, `bill_sepa_direct_debit_batch`).
- Native egui 0.37 desktop workstation (`billcraft-app`) with `--smoke` verification.
- Central fleet launcher `mcp-central-docs\starts\billcraft-start.bat`.
