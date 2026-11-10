---
updated: 2026-11-10T03:00:00+01:00
agent: antigravity
status: completed
tags: [billcraft, invoicing, ebinterface, sepa, craft-biz]
---

# Handover — BillCraft

## What Was Done
- Shipped full 4-crate pure Rust workspace (`billcraft-store`, `billcraft-core`, `billcraft-mcp`, `billcraft-app`).
- Implemented Austrian ebInterface 6.1 XML generator and ISO 20022 SEPA Direct Debit (`pain.008.001.02`) XML batch creator.
- Added subscription recurring billing and event bus integration (`craft-biz::bus::Bus`).
- Native desktop egui 0.37 UI with live table, customer drawer, and `--smoke` check.
- Clean `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` compilation with 0 warnings.
- Pushed to GitHub at `https://github.com/sandraschi/billcraft`.

## Last Concrete Action
- Verified `mcp-central-docs\starts\billcraft-start.bat --smoke` exits with code 0 (`smoke: ok (billcraft-app)`).

## Next Step
- Conduct pilot run with live customer accounts and Erste Bank Telebanking Direct Debit upload.
