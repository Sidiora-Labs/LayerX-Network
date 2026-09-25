import assert from "node:assert/strict";
import test from "node:test";

import { copyEntries } from "../copy/catalog.ts";
import { copyEntry } from "../copy/runtime.ts";
import {
  EXPLORER_ANCHOR_PATH,
  EXPLORER_LINK_KINDS,
  EXPLORER_NAVIGATION,
  explorerLinkPath,
  type ExplorerLinkTarget,
} from "../src/explorer/links.ts";
import { ROUTE_SCRIPT_BUDGETS } from "../src/perf/budgets.ts";

const RECEIPT = "a".repeat(64);
const TRANSACTION = `0x${"b".repeat(64)}`;
const ADDRESS = `0x${"1".repeat(40)}`;

const RETIRED_ROUTES = [
  "/explorer/batches",
  "/explorer/batches/[batchNumber]",
  "/explorer/checkpoints",
  "/explorer/checkpoints/[checkpointId]",
] as const;

const KEPT_EXPLORER_ROUTES = [
  "/explorer",
  "/explorer/accounts/[accountId]",
  "/explorer/programs/[programId]",
  "/explorer/receipts/[receiptId]",
  "/explorer/verify",
] as const;

function budgetedRoutes(): readonly string[] {
  return Object.keys(ROUTE_SCRIPT_BUDGETS);
}

test("every anchor target resolves to the explorer's one anchor surface", () => {
  for (const target of [{ kind: "anchor" }, { kind: "batch" }, { kind: "checkpoint" }] as const) {
    assert.equal(explorerLinkPath(target), EXPLORER_ANCHOR_PATH);
  }
  assert.equal(EXPLORER_ANCHOR_PATH, "/paxeer-x/anchors");
});

test("the identified targets carry their identifier into the explorer's own route", () => {
  assert.equal(
    explorerLinkPath({ kind: "receipt", receiptId: RECEIPT.toUpperCase() }),
    `/paxeer-x/receipts/${RECEIPT}`,
  );
  assert.equal(
    explorerLinkPath({ kind: "transaction", transactionHash: `0x${"B".repeat(64)}` }),
    `/tx/${TRANSACTION}`,
  );
  assert.equal(explorerLinkPath({ kind: "address", address: ADDRESS }), `/address/${ADDRESS}`);
});

test("the link surface covers every explorer link kind and refuses identifiers it cannot address", () => {
  assert.deepEqual([...EXPLORER_LINK_KINDS], [
    "anchor",
    "batch",
    "checkpoint",
    "receipt",
    "transaction",
    "address",
  ]);
  const refused: readonly ExplorerLinkTarget[] = [
    { kind: "receipt", receiptId: "" },
    { kind: "receipt", receiptId: RECEIPT.slice(1) },
    { kind: "receipt", receiptId: `${RECEIPT}f` },
    { kind: "transaction", transactionHash: RECEIPT },
    { kind: "transaction", transactionHash: `0x${"b".repeat(63)}` },
    { kind: "address", address: `0x${"1".repeat(39)}` },
    { kind: "address", address: ADDRESS.slice(2) },
  ];
  for (const target of refused) {
    assert.throws(() => explorerLinkPath(target), TypeError, JSON.stringify(target));
  }
});

test("a link path never carries a host of its own", () => {
  const targets: readonly ExplorerLinkTarget[] = [
    { kind: "anchor" },
    { kind: "batch" },
    { kind: "checkpoint" },
    { kind: "receipt", receiptId: RECEIPT },
    { kind: "transaction", transactionHash: TRANSACTION },
    { kind: "address", address: ADDRESS },
  ];
  for (const target of targets) {
    const path = explorerLinkPath(target);
    assert.ok(path.startsWith("/"), path);
    assert.doesNotMatch(path, /^\/\//u, path);
    assert.doesNotMatch(path, /:\/\//u, path);
  }
});

test("the explorer navigation names only routes the control plane still serves", () => {
  const navigation: readonly string[] = EXPLORER_NAVIGATION.map((item) => item.href);
  assert.deepEqual(navigation, ["/explorer", "/explorer/verify"]);
  for (const item of EXPLORER_NAVIGATION) {
    assert.ok(budgetedRoutes().includes(item.href), item.href);
    assert.equal(copyEntry(item.copyKey).key, item.copyKey);
    assert.ok(copyEntry(item.copyKey).message.length > 0, item.copyKey);
  }
  for (const retired of RETIRED_ROUTES) {
    assert.ok(!navigation.includes(retired), `navigation still names ${retired}`);
  }
});

test("the route performance budgets name the routes that remain and no retired one", () => {
  const routes = budgetedRoutes();
  for (const retired of RETIRED_ROUTES) {
    assert.ok(!routes.includes(retired), `budgets still name ${retired}`);
  }
  assert.deepEqual(routes.filter((route) => route.startsWith("/explorer")), [...KEPT_EXPLORER_ROUTES]);
});

test("the copy catalogue carries the link panel and nothing written for a retired page", () => {
  for (const key of ["explorer.anchors.title", "explorer.anchors.body", "explorer.anchors.action"]) {
    assert.ok(copyEntry(key).message.length > 0, key);
  }
  const retiredKeys = [
    "explorer.navigation.batches",
    "explorer.navigation.checkpoints",
    "explorer.batches.title",
    "explorer.batches.body",
    "explorer.batches.recent",
    "explorer.batches.table",
    "explorer.batch.title",
    "explorer.batch.facts",
    "explorer.checkpoints.title",
    "explorer.checkpoints.body",
    "explorer.checkpoints.recent",
    "explorer.checkpoints.table",
    "explorer.checkpoint.title",
    "explorer.checkpoint.facts",
    "explorer.fact.first_sequence",
    "explorer.fact.last_sequence",
    "explorer.column.activities",
    "explorer.column.bytes",
    "explorer.column.checkpoint",
    "explorer.column.sequences",
    "explorer.column.signatures",
  ];
  for (const key of retiredKeys) {
    assert.ok(copyEntries.every((entry) => entry.key !== key), `the catalogue still carries ${key}`);
    assert.throws(() => copyEntry(key), /Unknown copy key/u, key);
  }
});

test("the surfaces the beta contract requires of the control plane are kept", () => {
  const routes = budgetedRoutes();
  for (const required of KEPT_EXPLORER_ROUTES) {
    assert.ok(routes.includes(required), required);
  }
  for (const key of [
    "explorer.lookup.receipt.title",
    "explorer.lookup.account.title",
    "explorer.lookup.program.title",
    "explorer.lookup.action",
    "explorer.verify.title",
    "explorer.account.title",
    "explorer.receipt.title",
    "explorer.program.title",
  ]) {
    assert.ok(copyEntry(key).message.length > 0, key);
  }
});
