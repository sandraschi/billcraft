# BillCraft PRD

Status 13 November 2026. This file is the PRD. The fleet page that graded the app A- is an assessment, and that grade is withdrawn.

## What it does

- SQLite invoices, items, customers, subscriptions.
- Issue, mark paid, flag overdue. Issuing emits `invoice.issued` toward GainCraft.
- `export_ebinterface` writes an ebInterface 6.1-shaped document. Tested.
- `export_sepa_direct_debit` writes pain.008.001.02. Tested.
- `export_xrechnung` writes a UBL 2.1 invoice for XRechnung 3.0.2 from the invoice lines plus a stored seller party (`seller`) and a buyer party (the invoice id). CLI: `party`, then `export-xrechnung`. The store test runs KoSIT Validator 1.6.3 with the 2026-08-31 configuration. That fixture was accepted. EndpointID is the Peppol participant id, not the email. A missing party, VAT id, phone, IBAN, buyer reference, or participant id is an error.
- `peppol-send` wraps that invoice in an SBDH and posts one AS4 message when `BILLCRAFT_AP_CERT`, `BILLCRAFT_AP_KEY`, and `BILLCRAFT_SMP_URL` are all set. A receipt for that message id marks the outbox `accepted`. Anything else marks it `failed`. If any of the three is missing, the package stays `prepared` and nothing is sent. The window button calls the same function. This is a sender that uses the firm's own access-point certificate. It does not register an access point and it does not receive invoices. A live OpenPeppol testbed receipt is still the proof that a buyer got the invoice. What Peppol is, and what is the rented SaaS around it, is in `mcp-central-docs` `projects/craft-fleet-expansion/PEPPOL.md`.
- `drain_inbox` drafts an invoice from `contract.signed` or `timesheet.approved` only when the event carries a positive cent amount.
- The window menu is File, Edit, View, Window, and Help. `--smoke` checks it.

## What it does not do yet

- Claim a buyer received an invoice before a live OpenPeppol testbed receipt.
- Operate a Peppol access point: no SML registration, no inbound invoices.
- Replace Stripe Billing or Chargebee.

## Done when

The bus drain, the KoSIT fixture, and the offline AS4 stub are in. A live testbed receipt is not. Access-point operation is not. See `BACKOFFICE_THIN_RUNT_PLAN.md` and `PEPPOL.md`.
