# BillCraft 🧾

**Local-first sovereign invoicing, subscription recurring billing, and European e-invoicing engine.**

Part of the Sovereign Back-Office Suite replacing SaaS spend with zero-cloud-dependency, instant, offline-capable Rust native applications.

## Features

- **Austrian ebInterface 6.1**: Full compliance with Austrian federal e-invoicing standards (`ebInterface:Invoice` XML).
- **EU XRechnung / Peppol Ready**: Compliant XML document generation for B2G European invoicing.
- **ISO 20022 SEPA Direct Debit (`pain.008.001.02`)**: Automated direct debit batch XML generation with Austrian creditor identifiers.
- **Subscription Engine**: Automated monthly and annual recurring invoice generation.
- **Event Bus Integration**: Drains `contract.signed` from `contractcraft` and `timesheet.approved` from `teamcraft`; emits `invoice.issued`, `invoice.paid`, and `invoice.overdue`.
- **Tri-Modal Interface**:
  - **L1 CLI**: Scriptable automation.
  - **L2 MCP**: 8 FastMCP tools for AI agent orchestration.
  - **L3 GUI**: High-performance native desktop UI via `egui 0.37`.

## Architecture

- `crates/billcraft-store`: Pure SQLite database layer with transactional integrity.
- `crates/billcraft-core`: XML generator (ebInterface 6.1, SEPA pain.008), recurring billing logic, bus handler.
- `crates/billcraft-mcp`: Stdio JSON-RPC MCP server & CLI binary (`billcraft`).
- `crates/billcraft-app`: Native desktop GUI executable (`billcraft-app`).

## Quick Start

```powershell
# Run smoke tests
.\start.ps1 -Smoke

# Launch native desktop app
.\start.ps1

# Run MCP server on stdio
.\start.ps1 -Mcp
```

## License

MIT OR Apache-2.0
