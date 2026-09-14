import assert from "node:assert/strict";
import test from "node:test";

import { nativeFeeBudget, nativeFeeBudgetIdentity, parseFeeAmount } from "../src/auth/native-fee-budget.ts";
import { decodeNativeFeeBudget, encodeNativeFeeBudget } from "../src/api/index.ts";

const asset = {
  asset_id: "b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898",
  currency: "LXT", decimals: 18,
};

test("fee consent preserves every base unit through the generated public contract", () => {
  const budget = nativeFeeBudget({
    perAction: "0.000000000000000001", total: "1.000000000000000001", perPeriod: "0.25",
  }, asset);
  assert.ok(budget);
  assert.deepEqual(encodeNativeFeeBudget(budget), {
    asset_id: asset.asset_id,
    maximum_per_activity: "1",
    maximum_total: "1000000000000000001",
    period_length_ms: "86400000",
    maximum_per_period: "250000000000000000",
  });
  assert.deepEqual(decodeNativeFeeBudget(encodeNativeFeeBudget(budget), "consent"), budget);
});

test("fee limits refuse rounding, overflow, missing consent and mismatched ranges", () => {
  for (const input of ["", "0", "-1", "1e3", "1,000", "01", "Infinity", "0.0000000000000000001"]) {
    assert.equal(parseFeeAmount(input, 18), undefined, input);
  }
  assert.equal(parseFeeAmount("340282366920938463463374607431768211456", 0), undefined);
  assert.equal(parseFeeAmount("340282366920938463463374607431768211455", 0), (1n << 128n) - 1n);
  assert.equal(parseFeeAmount("1.1", 0), undefined);
  const valid = { perAction: "1", total: "10", perPeriod: "2" };
  assert.equal(nativeFeeBudget(valid, undefined), undefined);
  assert.equal(nativeFeeBudget(valid, { ...asset, asset_id: "0".repeat(64) }), undefined);
  assert.equal(nativeFeeBudget(valid, { ...asset, decimals: 39 }), undefined);
  assert.equal(nativeFeeBudget({ ...valid, total: "" }, asset), undefined);
  assert.equal(nativeFeeBudget({ ...valid, perAction: "11" }, asset), undefined);
  assert.equal(nativeFeeBudget({ ...valid, perPeriod: "0.5" }, asset), undefined);
});

test("changing any approved fee limit changes the operation identity", () => {
  const original = nativeFeeBudget({ perAction: "1", total: "10", perPeriod: "2" }, asset);
  assert.ok(original);
  for (const changed of [
    { ...original, asset_id: "1".repeat(64) },
    { ...original, maximum_per_activity: 2n },
    { ...original, maximum_total: 2n },
    { ...original, maximum_per_period: 2n },
    { ...original, period_length_ms: 2n },
  ]) {
    assert.notEqual(nativeFeeBudgetIdentity(changed), nativeFeeBudgetIdentity(original));
  }
});
