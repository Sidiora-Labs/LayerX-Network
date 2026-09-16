# Market-maker ramps

Independent market-maker ramps are **not** an `interop/crates`
adapter. They live under `platform/ramps/`: toolkit crate
`layerx-ramp-toolkit` (`platform/ramps/toolkit`) and reference
material under `platform/ramps/reference/`
(`platform/ramps/README.md`).

`spec/layerx-platform/spec.kvx` task 26.1 is **implemented**. The
spec `reality` field records that the reference ramp service is
incomplete. Task 26.2 (labelling) is **implemented** with
qualification still pending. This page does not claim a finished
hosted ramp.

---

## Role

Market makers act as ordinary LayerX principals. External custody is
labelled `EXTERNAL_CUSTODY_LABEL`. Orders are receipt-gated. The
journal domain is `LXP/market-maker-ramp/journal/v1`.

---

## Toolkit surface

From `platform/ramps/toolkit/src/lib.rs`:

`RampDirection`, `QuoteTerms`, `RampOrder`,
`compile_payer_grant_draw`, `compile_operator_send`,
`verify_order_receipt`, `RampPresentation`, `RampError`,
`platform_ramp_toolkit()`, plus modules `clients`, `engine`,
`journal`.

---

## Qualification gates

Makefile targets named in the spec and `platform/ramps/README.md`:

- `make interop-test-ramps`
- `make interop-test-ramps-sandbox` (requires `LAYERX_RAMP_URL`)

Fiat provider callbacks at the interop edge are
[Fiat ramps](FiatRamps.md).

[Interop](Interop.md) · [Home](Home.md)
