# BillCraft PRD

Status 13 November 2026. This file is the PRD. The fleet page that graded the app A- is an assessment, and that grade is withdrawn.

## What it does

- SQLite invoices, items, customers, subscriptions.
- Issue, mark paid, flag overdue. Issuing emits `invoice.issued` toward GainCraft.
- `export_ebinterface` writes an ebInterface 6.1-shaped document. Tested.
- `export_sepa_direct_debit` writes pain.008.001.02. Tested.
- `export_xrechnung` writes a UBL 2.1 invoice for XRechnung 3.0.2 from the invoice lines plus a stored seller party (`seller`) and a buyer party (the invoice id). CLI: `party`, then `export-xrechnung`. The store test runs KoSIT Validator 1.6.3 with the 2026-08-31 configuration. That fixture was accepted. It is not a Peppol AS4 node. A missing party, VAT id, phone, IBAN, or buyer reference is an error.
- `drain_inbox` drafts an invoice from `contract.signed` or `timesheet.approved` only when the event carries a positive cent amount.
- The window menu is File, Edit, View, Window, and Help. `--smoke` checks it.
- The window menu is File, Edit, View, Window, and Help. `--smoke` checks it.

## What it does not do yet

- Send the invoice over Peppol.
- Replace Stripe Billing or Chargebee.

## Done when

The bus drain and the KoSIT fixture are in. Peppol delivery is not. See `BACKOFFICE_THIN_RUNT_PLAN.md`.
