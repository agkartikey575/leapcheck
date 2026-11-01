# Roadmap — Staff Backend Engineer

Owner agent: `backend-engineer` · Reports to: Chief Technology Officer

Status key: ⬜ todo · 🟨 in progress · ✅ done · ❌ dropped

## Mission
Build the converter and webhook pipeline so every finalized Stripe invoice becomes a valid e-invoice.

## KPIs
- Test coverage on core: _baseline TBD_
- Validation pass rate on fixtures: _baseline TBD_
- Webhook failure rate: _baseline TBD_

## Q4 2026
- ⬜ einvoice-core CII + UBL generation
- ⬜ Fixture tests green against KoSIT

## Q1 2027
- ⬜ Webhook pipeline
- ⬜ PDF/A-3 output
- ⬜ Storage + download links

## Q2 2027
- ⬜ PA API client

## H2 2027
- ⬜ Peppol partner client

## Idea backlog
- A CLI `npx einvoice-check <stripe-invoice-id>` for developers — growth loop via GitHub.
