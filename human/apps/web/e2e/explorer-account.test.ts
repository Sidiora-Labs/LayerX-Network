import assert from "node:assert/strict";
import test from "node:test";

import {
  accountIdentifierPath,
  decodeUnifiedAccount,
  parseAccountIdentifier,
} from "../src/explorer/model.ts";

const ADDRESS = "0x1111111111111111111111111111111111111111";
const ACCOUNT = "a".repeat(64);
const ASSET = "b".repeat(64);
const TRANSACTION = `0x${"c".repeat(64)}`;

function unifiedDocument(): Record<string, unknown> {
  return {
    requested: ADDRESS,
    canonical: ACCOUNT,
    evidence: "gateway-reported",
    identities: {
      evm_address: ADDRESS,
      pax_address: "pax1qqqqq",
      layerx_did: `did:layerx:${ACCOUNT}`,
      layerx_account: ACCOUNT,
      bound: true,
    },
    balances: {
      joined_limit: "1024",
      items: [{ asset_id: ASSET, denom: "PAXD", custody: "7", paxeer: "5", layerx: "2" }],
    },
    settlement: {
      network_id: "paxeer-x",
      chain_id: "9999",
      instant_block: "120",
      sealed_batch: "41",
      finalized_batch: "40",
      anchor_status: "2",
      anchor_status_name: "final",
    },
    paxeer_activity: {
      from_block: "70",
      to_block: "120",
      next_before_block: "70",
      items: [
        {
          event: "custody-deposit",
          block_number: "119",
          log_index: "3",
          transaction_hash: TRANSACTION,
          asset_id: ASSET,
          amount: "5",
          address: ADDRESS,
          account: ACCOUNT,
        },
      ],
    },
  };
}

test("every spelling of one account normalises to a single page identifier", () => {
  assert.deepEqual(
    parseAccountIdentifier(`  ${ADDRESS.toUpperCase()} `),
    { kind: "evm", canonical: ADDRESS },
  );
  assert.deepEqual(
    parseAccountIdentifier(`DID:LAYERX:${ACCOUNT.toUpperCase()}`),
    { kind: "did", canonical: `did:layerx:${ACCOUNT}` },
  );
  assert.deepEqual(
    parseAccountIdentifier(ACCOUNT.toUpperCase()),
    { kind: "account", canonical: ACCOUNT },
  );
  assert.equal(accountIdentifierPath(ACCOUNT), `/explorer/accounts/${ACCOUNT}`);
  assert.equal(
    accountIdentifierPath(`did:layerx:${ACCOUNT}`),
    `/explorer/accounts/did%3Alayerx%3A${ACCOUNT}`,
  );
});

test("identifiers that name no account of this network are refused", () => {
  for (const candidate of [
    "",
    "0x1234",
    `0x${"1".repeat(41)}`,
    "did:layerx:",
    `did:layerx:${"z".repeat(64)}`,
    `did:web:${ACCOUNT}`,
    ACCOUNT.slice(1),
    `${ACCOUNT}f`,
  ]) {
    assert.equal(parseAccountIdentifier(candidate), undefined, candidate);
  }
});

test("a unified account answer decodes both halves of the account", () => {
  const account = decodeUnifiedAccount(unifiedDocument());
  assert.equal(account.requested, ADDRESS);
  assert.equal(account.canonical, ACCOUNT);
  assert.equal(account.evidence, "gateway-reported");
  assert.deepEqual(account.identities, {
    evmAddress: ADDRESS,
    paxAddress: "pax1qqqqq",
    layerxDid: `did:layerx:${ACCOUNT}`,
    layerxAccount: ACCOUNT,
    bound: true,
  });
  assert.deepEqual(account.balances.items, [
    { assetId: ASSET, denom: "PAXD", custody: "7", paxeer: "5", layerx: "2" },
  ]);
  assert.equal(account.balances.joinedLimit, "1024");
  assert.equal(account.settlement.instantBlock, "120");
  assert.equal(account.settlement.sealedBatch, "41");
  assert.equal(account.settlement.finalizedBatch, "40");
  assert.equal(account.settlement.anchorStatusName, "final");
  assert.equal(account.paxeerActivity.fromBlock, "70");
  assert.equal(account.paxeerActivity.toBlock, "120");
  assert.equal(account.paxeerActivity.nextBeforeBlock, "70");
  assert.deepEqual(account.paxeerActivity.items, [
    {
      event: "custody-deposit",
      blockNumber: "119",
      logIndex: "3",
      transactionHash: TRANSACTION,
      assetId: ASSET,
      amount: "5",
      address: ADDRESS,
      account: ACCOUNT,
    },
  ]);
});

test("an account with only one half decodes without inventing the other", () => {
  const document = unifiedDocument();
  document.canonical = ADDRESS;
  document.identities = { evm_address: ADDRESS, bound: false };
  document.paxeer_activity = { from_block: "70", to_block: "120", items: [] };
  const account = decodeUnifiedAccount(document);
  assert.deepEqual(account.identities, { evmAddress: ADDRESS, bound: false });
  assert.equal(account.canonical, ADDRESS);
  assert.equal(account.paxeerActivity.items.length, 0);
  assert.equal(account.paxeerActivity.nextBeforeBlock, undefined);
});

test("unified answers that are not gateway-reported or well formed are refused", () => {
  const unsigned = unifiedDocument();
  unsigned.evidence = "proof-verified";
  assert.throws(() => decodeUnifiedAccount(unsigned), TypeError);

  const foreignEvent = unifiedDocument();
  foreignEvent.paxeer_activity = {
    from_block: "70",
    to_block: "120",
    items: [{
      event: "airdrop",
      block_number: "119",
      log_index: "3",
      transaction_hash: TRANSACTION,
    }],
  };
  assert.throws(() => decodeUnifiedAccount(foreignEvent), TypeError);

  const unboundedBalances = unifiedDocument();
  unboundedBalances.balances = {
    joined_limit: "1024",
    items: Array.from({ length: 1_025 }, () => ({ asset_id: ASSET, denom: "PAXD" })),
  };
  assert.throws(() => decodeUnifiedAccount(unboundedBalances), TypeError);

  const unboundedActivity = unifiedDocument();
  unboundedActivity.paxeer_activity = {
    from_block: "70",
    to_block: "120",
    items: Array.from({ length: 101 }, () => ({
      event: "custody-deposit",
      block_number: "119",
      log_index: "3",
      transaction_hash: TRANSACTION,
    })),
  };
  assert.throws(() => decodeUnifiedAccount(unboundedActivity), TypeError);

  const foreignAccount = unifiedDocument();
  foreignAccount.canonical = "not-an-account";
  assert.throws(() => decodeUnifiedAccount(foreignAccount), TypeError);

  const badAddress = unifiedDocument();
  badAddress.identities = { evm_address: "0x00", bound: false };
  assert.throws(() => decodeUnifiedAccount(badAddress), TypeError);
});
