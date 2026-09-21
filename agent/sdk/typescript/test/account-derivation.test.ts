import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { secp256k1 } from "@noble/curves/secp256k1.js";

import {
  AccountDerivationError,
  LAYERX_COIN_TYPE,
  autoBindLayerX,
  bindNonceCall,
  decodeBindNonce,
  decodeBoundDid,
  deriveFromBrowserWallet,
  deriveFromMnemonic,
  deriveFromWalletSignature,
  keyDerivationHash,
  keyDerivationRequest,
  keyDerivationTypedData,
  normalizeWalletSignature,
  planLayerXBind,
  signBindTransaction,
  slip10Ed25519,
  type Eip1193Requester,
} from "../src/index.js";

const fixture = JSON.parse(
  readFileSync(new URL("../../../../../platform/sdk/conformance/fixtures/account-derivation-v1.json", import.meta.url), "utf8"),
);
const hex = (bytes: Uint8Array): string => Buffer.from(bytes).toString("hex");
const bytes = (text: string): Uint8Array => new Uint8Array(Buffer.from(text, "hex"));
const refuses = (code: string, run: () => unknown): void => {
  assert.throws(run, (error: unknown) => error instanceof AccountDerivationError && error.code === code);
};

// SLIP-0010 official Ed25519 test vector 1.
const slipSeed = bytes("000102030405060708090a0b0c0d0e0f");
assert.equal(hex(slip10Ed25519(slipSeed, [])), "2b4be7f19ee27bbf30c667b642d5f4aa69fd169872f8fc3059c08ebae2eb19e7");
assert.equal(hex(slip10Ed25519(slipSeed, [0, 1])), "b1d0bad404bf35da785a64ca1ac54b2617211d2777696fbffaf208f746ae84f2");
assert.equal(
  hex(slip10Ed25519(slipSeed, [0, 1, 2, 2, 1000000000])),
  "8f94d394a8e8fd6b1bc2f3f49f5c47e385281d5c17e65324b0f62483e37e8793",
);

assert.equal(fixture.layerx_coin_type, LAYERX_COIN_TYPE);
let seen = 0;
for (const vector of fixture.mnemonic_vectors) {
  for (const expected of vector.accounts) {
    const derived = deriveFromMnemonic(`  ${vector.mnemonic.replaceAll(" ", "  ")}\n`, {
      passphrase: vector.passphrase,
      index: expected.index,
    });
    assert.equal(derived.evmAddress, expected.evm_address);
    assert.equal(derived.layerxPublicKey, expected.layerx_public_key);
    assert.equal(derived.did, expected.did);
    const plan = planLayerXBind(derived, expected.bind.chain_id, { boundDidPublicKey: null, nonce: BigInt(expected.bind.nonce) });
    assert.equal(plan.action, "bind");
    if (plan.action === "bind") {
      assert.equal(plan.call.data, `0x${expected.bind.calldata}`);
      assert.equal(plan.call.to, expected.bind.to);
      const transaction = expected.bind.transaction;
      assert.equal(
        signBindTransaction(derived, expected.bind.chain_id, plan.call, {
          evmNonce: BigInt(transaction.evm_nonce),
          maxPriorityFeePerGas: BigInt(transaction.max_priority_fee_per_gas),
          maxFeePerGas: BigInt(transaction.max_fee_per_gas),
          gasLimit: BigInt(transaction.gas_limit),
        }),
        `0x${transaction.raw}`,
      );
    }
    assert.deepEqual(planLayerXBind(derived, 713714, { boundDidPublicKey: derived.layerxPublicKey.toUpperCase(), nonce: 1n }), {
      action: "already_bound",
    });
    refuses("bound_to_different_did", () => planLayerXBind(derived, 713714, { boundDidPublicKey: "aa".repeat(32), nonce: 1n }));
    seen += 1;
  }
}
assert.equal(seen, 4);
assert.equal(
  deriveFromMnemonic("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about").evmAddress,
  "0x9858EfFD232B4033E47d90003D41EC34EcaEda94",
);
refuses("invalid_mnemonic", () => deriveFromMnemonic("abandon abandon abandon"));
refuses("invalid_mnemonic", () => deriveFromMnemonic(`${"abandon ".repeat(11)}abandon`));
refuses("index_out_of_range", () => deriveFromMnemonic(fixture.mnemonic_vectors[0].mnemonic, { index: 0x80000000 }));

const wallet = fixture.wallet_signature;
for (const vector of wallet.vectors) {
  const parameters = { chainId: fixture.chain_id, address: wallet.evm_address, index: vector.index };
  assert.deepEqual(keyDerivationTypedData(parameters), vector.typed_data);
  assert.equal(hex(keyDerivationHash(parameters)), vector.eip712_hash);
  const request = keyDerivationRequest(parameters);
  assert.equal(request.method, "eth_signTypedData_v4");
  assert.equal(request.params[0], wallet.evm_address.toLowerCase());
  assert.deepEqual(JSON.parse(request.params[1] as string), vector.typed_data);

  const produced = secp256k1.sign(keyDerivationHash(parameters), bytes(wallet.private_key), { prehash: false, format: "recovered" });
  assert.equal(hex(produced.slice(1)) + (27 + (produced[0] ?? 0)).toString(16), vector.signature);

  const derived = deriveFromWalletSignature(parameters, `0x${vector.signature}`);
  assert.equal(derived.did, vector.did);
  assert.equal(derived.evmAddress, wallet.evm_address);
  assert.equal(derived.evmPrivateKey, null);
  for (const equivalent of vector.equivalent_signatures) {
    assert.notEqual(equivalent, vector.signature);
    assert.equal(hex(normalizeWalletSignature(equivalent)), vector.signature);
    assert.equal(deriveFromWalletSignature(parameters, bytes(equivalent)).did, vector.did);
  }
  refuses("wallet_signer_mismatch", () => deriveFromWalletSignature({ ...parameters, chainId: fixture.chain_id + 1 }, vector.signature));
  refuses("invalid_wallet_signature", () => deriveFromWalletSignature(parameters, `${vector.signature.slice(0, 128)}1d`));
  refuses("invalid_origin", () => keyDerivationTypedData({ ...parameters, origin: 'https://Evil.example/"' }));
  refuses("evm_key_unavailable", () =>
    signBindTransaction(derived, 713714, { to: wallet.evm_address, data: "0x", didPublicKey: "", signature: "", nonce: 0n }, {
      evmNonce: 0n,
      maxPriorityFeePerGas: 1n,
      maxFeePerGas: 1n,
      gasLimit: 1n,
    }),
  );

  // Browser flow: an in-process RFC 6979 signer holding the test key is asked twice and accepted.
  const asked: string[] = [];
  const deterministic: Eip1193Requester = {
    request: async ({ method, params }) => {
      asked.push(method);
      assert.deepEqual(JSON.parse(params?.[1] as string), vector.typed_data);
      const signature = secp256k1.sign(keyDerivationHash(parameters), bytes(wallet.private_key), { prehash: false, format: "recovered" });
      return `0x${hex(signature.slice(1))}${(27 + (signature[0] ?? 0)).toString(16)}`;
    },
  };
  assert.equal((await deriveFromBrowserWallet(deterministic, parameters)).did, vector.did);
  assert.deepEqual(asked, ["eth_signTypedData_v4", "eth_signTypedData_v4"]);

  // A wallet that signs with a fresh nonce each time is refused.
  let extra = 1n;
  const randomised: Eip1193Requester = {
    request: async () => {
      extra += 1n;
      const signature = secp256k1.sign(keyDerivationHash(parameters), bytes(wallet.private_key), {
        prehash: false,
        format: "recovered",
        extraEntropy: bytes(extra.toString(16).padStart(64, "0")),
      });
      return `0x${hex(signature.slice(1))}${(27 + (signature[0] ?? 0)).toString(16)}`;
    },
  };
  await assert.rejects(
    deriveFromBrowserWallet(randomised, parameters),
    (error: unknown) => error instanceof AccountDerivationError && error.code === "wallet_signature_not_deterministic",
  );
}

// Auto-bind against a wallet answering the two precompile reads.
const derived = deriveFromMnemonic(fixture.mnemonic_vectors[0].mnemonic);
const expected = fixture.mnemonic_vectors[0].accounts[0];
const chain = (bound: string | null, sent: unknown[]): Eip1193Requester => ({
  request: async ({ method, params }) => {
    if (method === "eth_sendTransaction") {
      sent.push(params?.[0]);
      return `0x${"12".repeat(32)}`;
    }
    const call = params?.[0] as { to: string; data: string };
    assert.equal(method, "eth_call");
    assert.equal(call.to, expected.bind.to);
    if (call.data === bindNonceCall(derived.evmAddress)) {
      return `0x${"00".repeat(32)}`;
    }
    return `0x${"00".repeat(64)}${bound ?? "00".repeat(32)}${"00".repeat(32)}`;
  },
});
const sent: unknown[] = [];
const outcome = await autoBindLayerX(chain(null, sent), derived, 713714);
assert.equal(outcome.action, "bind");
assert.deepEqual(sent, [{ from: derived.evmAddress, to: expected.bind.to, data: `0x${expected.bind.calldata}`, value: "0x0" }]);
const again: unknown[] = [];
assert.deepEqual(await autoBindLayerX(chain(derived.layerxPublicKey, again), derived, 713714), { action: "already_bound" });
await assert.rejects(
  autoBindLayerX(chain("bb".repeat(32), again), derived, 713714),
  (error: unknown) => error instanceof AccountDerivationError && error.code === "bound_to_different_did",
);
assert.deepEqual(again, []);
assert.equal(decodeBindNonce(`0x${"00".repeat(31)}07`), 7n);
assert.equal(decodeBoundDid(`0x${"00".repeat(128)}`), null);
refuses("malformed_precompile_answer", () => decodeBindNonce("0x01"));
refuses("malformed_precompile_answer", () => decodeBoundDid(`0x${"00".repeat(64)}`));

console.log("account derivation: ok");
