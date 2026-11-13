---
updated: 2026-11-13T02:10:00+01:00
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
- `export_xrechnung` was accepted by KoSIT Validator 1.6.3 with the XRechnung 3.0.2 configuration of 31 August 2026. CLI: `party` then `export-xrechnung`.

## Next Step
- The thin-plan queue is empty. The menu bar is on the window.
