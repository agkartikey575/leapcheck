# Product Scope — v1 ("Rechnung for Stripe", working name)

## Problem
EU businesses invoicing through Stripe must issue structured e-invoices (Germany 2027–28, France
Sept 2027). Stripe doesn't produce them. Today their options are: switch invoicing software,
hire a developer, or do it by hand. All are painful.

## Customer (ICP)
German-based SaaS companies, agencies, consultants and online sellers with 10–2,000 Stripe
invoices/month, no in-house finance engineer. Secondary: their accountants (Steuerberater), who
recommend tools to many clients.

## v1 — must have (target: 4–6 weeks)
1. **Stripe App** installed from the Stripe App Marketplace (OAuth, no code).
2. Listens to `invoice.finalized` webhook → builds an EN 16931 invoice from Stripe data
   (seller details, customer address, `tax_ids`, line items, tax rates, totals).
3. Outputs **ZUGFeRD/Factur-X (PDF/A-3 + embedded CII XML, EN16931 profile)** and **XRechnung (UBL)**.
4. **Validates every file** with the official KoSIT validator + XRechnung configuration before use.
5. Attaches the file to the Stripe invoice / customer email; stores it for download.
6. Settings panel in the Stripe Dashboard: seller data, Leitweg-ID for public buyers, format choice.
7. "Fix-it" warnings in plain language when data is missing (e.g. "customer has no address").
8. Billing via Stripe (naturally): free 10 invoices/mo → €19/mo (200) → €49/mo (1,000) → €129/mo (5,000).

## v1 — explicitly out of scope
- Sending over Peppol or French PA (needs certification → later via partners)
- Receiving/inbox for incoming e-invoices
- Non-Stripe platforms
- Legal/tax advice of any kind

## Lead magnet (week 3–4, parallel)
Free web validator + "is my Stripe invoice e-invoice-ready?" checker. SEO in German
("Stripe XRechnung", "Stripe E-Rechnung", "ZUGFeRD Stripe").

## Open-source core
`@<org>/einvoice-core` (TypeScript): Stripe Invoice JSON → EN16931 model → CII/UBL. MIT licensed.
Developers adopt it; businesses pay for the hosted, validated, no-code app.

## Phases after v1
- **Phase 2 (Q1–Q2 2027):** France — Factur-X via partner Plateforme Agréée API, before Sept 2027 deadline.
- **Phase 3 (H2 2027):** Belgium/Peppol via partner access point; maybe Poland KSeF.
- **Phase 4:** Accountant dashboard (one login, many client accounts) → channel growth.

## Architecture (low maintenance, low cost)
- Stripe App UI (Stripe's React UI extension SDK)
- Backend: TypeScript on Vercel serverless or Cloudflare Workers; Postgres (Neon/Supabase free tier)
- File generation in-process (CII/UBL XML + PDF/A-3 embedding)
- Validation: KoSIT validator (JVM) in a small container (Fly.io/Cloud Run, scale-to-zero)
- Storage: object storage (Vercel Blob / R2), EU region (GDPR)
- Estimated running cost at launch: **< €30/month**

## Success metrics
- 30 days after launch: 50 installs, 10 paying
- 6 months: 300 paying accounts ≈ €7–10k MRR
- Validation pass rate ≥ 99.5%; support tickets < 1 per 50 accounts/month

## Key risks
| Risk | Mitigation |
|---|---|
| Stripe launches native e-invoicing | Move fast; own FR/accountant channel; OSS core keeps value |
| Incorrect invoice = customer tax issue | Official validator on every file; clear ToS; no legal claims |
| Stripe app review delays | Submit early; build lead magnet meanwhile |
| Competitor already on Marketplace | Verify first (CPO-1); differentiate on DE+FR + accountants |
