import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { DEFAULT_PROTOCOL_VERSION, STATE_COMMITMENT_PROTOCOL_VERSION } from "@sidiora/layerx-sdk";
import { MiddlewareError } from "@sidiora/layerx-seller-middleware";
import { AgentMiddleware, verifyCommittedAgentPayment } from "../dist/index.js";

const bytes = (hex) => Uint8Array.from(Buffer.from(hex, "hex"));

async function loadFixture(name) {
  const fixture = JSON.parse(await readFile(new URL(`../../../sdk/conformance/fixtures/${name}`, import.meta.url), "utf8"));
  const batch = fixture.authorized_batch;
  const request = {
    amount: fixture.expected.amount,
    asset: batch.asset_hex,
    recipient: fixture.expected.to_hex,
  };
  return {
    protocolVersion: fixture.expected.protocol_version,
    evidence: {
      canonicalReceipt: bytes(fixture.canonical_receipt_hex),
      authorizedBatch: {
        batchId: bytes(batch.batch_id_hex),
        asset: bytes(batch.asset_hex),
        previousStateRoot: bytes(batch.previous_state_root_hex),
        resultingStateRoot: bytes(batch.resulting_state_root_hex),
        sequencerPublicKey: bytes(batch.sequencer_public_key_hex),
      },
    },
    request,
    reservation: {
      state: "committed",
      reservationId: fixture.expected.activity_id_hex,
      requestDigest: fixture.expected.receipt_digest_hex,
      amount: request.amount,
      asset: request.asset,
      receiptDigest: fixture.expected.receipt_digest_hex,
    },
  };
}

const stateCommitment = await loadFixture("receipt-positive-v3.json");
const legacyDefault = await loadFixture("receipt-positive-v2.json");

test("a committed agent payment verifies the protocol version its caller selected", async () => {
  assert.equal(stateCommitment.protocolVersion, STATE_COMMITMENT_PROTOCOL_VERSION);
  const verified = await verifyCommittedAgentPayment(
    stateCommitment.evidence,
    stateCommitment.reservation,
    stateCommitment.request,
    undefined,
    { protocolVersion: STATE_COMMITMENT_PROTOCOL_VERSION },
  );
  assert.equal(verified.kind, "verified");
  assert.equal(verified.verification.receipt.protocolVersion, STATE_COMMITMENT_PROTOCOL_VERSION);
});

test("a committed agent payment of another protocol version is refused", async () => {
  await assert.rejects(verifyCommittedAgentPayment(
    stateCommitment.evidence,
    stateCommitment.reservation,
    stateCommitment.request,
    undefined,
    { protocolVersion: DEFAULT_PROTOCOL_VERSION },
  ));
  await assert.rejects(verifyCommittedAgentPayment(
    legacyDefault.evidence,
    legacyDefault.reservation,
    legacyDefault.request,
    undefined,
    { protocolVersion: STATE_COMMITMENT_PROTOCOL_VERSION },
  ));
});

test("an agent configuration without a usable protocol version is refused by name", () => {
  for (const declared of [undefined, null, 0, 1, 4, "3", 3.5]) {
    assert.throws(
      () => new AgentMiddleware({ protocolVersion: declared }),
      (error) => error instanceof MiddlewareError && error.code === "missing-protocol-version",
      `declared protocol version ${String(declared)} must be refused by name`,
    );
  }
});
