# BillCraft — AGENTS.md

Local-first sovereign invoicing, subscription recurring billing, and European e-invoicing engine.

## Crates

- `crates/billcraft-store`: Pure SQLite database layer (`Invoices`, `Subscriptions`, `Customers`).
- `crates/billcraft-core`: ebInterface 6.1 XML, SEPA Direct Debit pain.008 XML, recurring billing engine.
- `crates/billcraft-mcp`: Stdio JSON-RPC FastMCP server & CLI binary (`billcraft`).
- `crates/billcraft-app`: Native desktop GUI executable (`billcraft-app`) via `egui 0.37`.

## Bus Events

- Inbound: `contract.signed` (ContractCraft), `timesheet.approved` (TeamCraft)
- Outbound: `invoice.issued`, `invoice.paid`, `invoice.overdue`

## Smoke Test

```powershell
.\start.ps1 -Smoke
```
