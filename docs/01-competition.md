# Competition Analysis — EU e-invoicing for small businesses

_Researched: 2026-11-01. Most sources are vendor pages; figures are unverified unless noted._

## Bottom line

**The generic "upload a PDF, get an e-invoice" idea is crowded.** Germany in particular already has
many free validators, free generators, WooCommerce plugins, Sheets/Excel add-ins and open-source
libraries. Building one more of those would be a race to the bottom.

**The gap we found: Stripe.** Stripe Billing/Invoicing does **not** generate compliant e-invoices
itself; Stripe sends merchants to App Marketplace partners. Its preferred partner (Billit) is
Peppol/Belgium-centric, and A-Cube covers Italy. We found **no Marketplace app clearly advertising
German XRechnung/ZUGFeRD or French Factur-X output.** Thousands of SaaS companies, agencies and
online sellers in DE/FR/BE invoice through Stripe and are being told by their accountants they must
switch. That is a desperate, paying, under-served buyer with a built-in distribution channel
(Stripe App Marketplace).

> Caveat: `marketplace.stripe.com` was not reachable from this session, so the listing check is
> based on Stripe's own docs and search results. Verify the live Marketplace before launch (task CPO-1).

## Landscape by channel

### Free online validators / generators (saturated — use only as an SEO lead magnet)
- zugferd-validator.de, Facturwise, Mustangserver, Aidoks, invoice-converter.com — free validation
- Docuflair, xrechnungs.de (AI PDF→XRechnung), getzugferd.com — free creation/preview

### WooCommerce (crowded)
| Product | Model |
|---|---|
| Germanized Pro | ~€79/yr, full ZUGFeRD/XRechnung |
| German Market | ~€75/yr |
| BelegFix / xrechnung.dev | free plugin, metered €19–€499/mo |
| invoice-api.xhub | free plugin, paid API |
| Agrunert | €0.10/invoice |
| E-Invoicing for WooCommerce | Factur-X/UBL/ZUGFeRD/XRechnung |

### Shopify (served, with a platform limitation)
- Sufio (established, Peppol), Invoice9 (free plan, no reviews), InvoiceHop (Pro plans)
- Key complaint: Shopify does not expose buyer VAT numbers to apps → Peppol invoices fail.
  This is Shopify's limit, not something we can fix. **Stripe does expose customer tax IDs.**

### Google Sheets / Excel (served)
- E-Invoice Creator (Workspace add-on), Vellonode, SmartTools VBA, IT-Service Ruhr add-ins

### Open source (good building blocks, not competitors for SMB buyers)
- Mustangproject (Java), KoSIT validator (official German rules), akretion/factur-x (Python),
  e-invoice-eu (TypeScript, from spreadsheet/JSON), @attestwire/stripe-einvoice (OSS CLI, Stripe→EN16931)
- attestwire is the closest technical competitor: a library for developers, not a no-code app.

### Country specifics that shape scope
- **Germany:** receiving mandatory since 2025; issuing phases in 2027 (larger firms) → 2028 (all).
  Email delivery of XRechnung/ZUGFeRD is allowed. **Best first market for a file-generating app.**
- **France:** issuing for SMEs from Sept 2027, but invoices must flow through a certified
  Plateforme Agréée (~149 exist). A generator alone is a "solution compatible"; it needs a PA
  connection. **Phase 2, via partner PA API.**
- **Belgium:** Peppol B2B live since Jan 2026. Needs an access point → partner. Billit already owns
  the Stripe slot here. **Phase 3 / maybe skip.**
- **Poland:** KSeF national system, micro-businesses Jan 2027; several free WooCommerce plugins
  exist; Shopify/Stripe lack native support. **Possible phase 3.**

## Positioning
"Every Stripe invoice, automatically legal in Germany (and soon France). Install in 2 minutes,
no code." Cheaper and simpler than switching invoicing software; more complete than a dev library.

## Sources
- https://stripe.com/en-at/guides/send-e-invoices-on-stripe-billing-through-app-marketplace-partners
- https://docs.stripe.com/invoicing/e-invoicing
- https://rechnungskit.de/en/guides/stripe-invoicing
- https://www.billit.eu/en-int/integrations/stripe-app
- https://github.com/attestwire/stripe-einvoice
- https://invoice-api.xhub.io/en/compare/woocommerce-xrechnung-plugin-comparison
- https://wordpress.org/plugins/belegfix-e-invoices-xrechnung-zugferd/
- https://qualimero.com/en/blog/woocommerce-invoice-creation-complete-guide-2025-e-invoice-zugferd
- https://apps.shopify.com/sufio/reviews
- https://zeppol.com/shopify-b2b-invoicing-eu/
- https://zugferd-validator.de/en/validate
- https://www.xrechnungs.de/en/pdf-zu-xrechnung-zugferd-konverter
- https://workspace.google.com/marketplace/app/einvoice_creator_view_and_generate_einvo/229106705932
- https://github.com/gflohr/e-invoice-eu
- https://www.novutech.com/news/e-invoicing-in-europe-overview-of-mandates-2025-2027
- https://qonto.com/fr/blog/gestion-entreprise/facturation/comment-choisir-plateforme-agreee
- https://www.wpdesk.pl/blog/krajowy-system-e-faktur-ksef-dla-woocommerce/
