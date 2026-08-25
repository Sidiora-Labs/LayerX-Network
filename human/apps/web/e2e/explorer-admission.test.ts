import assert from "node:assert/strict";
import test from "node:test";

import {
  MirrorOverloadedError,
  MirrorVerificationAdmission,
} from "../src/explorer/mirror-admission.ts";

test("mirror verification admission refuses overload and releases every slot", async () => {
  const admission = new MirrorVerificationAdmission(1);
  let release!: () => void;
  const blocked = new Promise<void>((resolve) => { release = resolve; });
  const first = admission.run(async () => blocked);
  assert.equal(admission.active(), 1);
  await assert.rejects(() => admission.run(async () => undefined), MirrorOverloadedError);
  assert.equal(admission.active(), 1);
  release();
  await first;
  assert.equal(admission.active(), 0);
  await assert.rejects(
    () => admission.run(async () => { throw new Error("operation failed"); }),
    /operation failed/u,
  );
  assert.equal(admission.active(), 0);
});
