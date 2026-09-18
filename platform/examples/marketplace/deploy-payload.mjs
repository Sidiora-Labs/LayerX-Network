import { decodeNativeProgramDeploy, encodeNativeProgramDeploy } from "@sidiora/layerx-sdk";
import { LayerXApplicationStateError, hex32 } from "../support/runtime.mjs";

export const IMMUTABLE_POLICY = 0;

export function canonicalNativeProgramDeploy({ programId, abiVersion, codeHash, wasm }) {
  if (![1, 2, 3].includes(abiVersion)) {
    throw new LayerXApplicationStateError("refused", "program_build_reported_unsupported_guest_abi");
  }
  const value = {
    programId: hex32(programId),
    guestAbi: abiVersion,
    policy: IMMUTABLE_POLICY,
    authority: new Uint8Array(32),
    newHash: hex32(codeHash),
    wasm: Uint8Array.from(wasm),
  };
  let payload;
  let decoded;
  try {
    payload = Uint8Array.from(encodeNativeProgramDeploy(value));
    decoded = decodeNativeProgramDeploy(payload);
  } catch {
    throw new LayerXApplicationStateError("refused", "program_deployment_is_not_canonical");
  }
  return Object.freeze({ payload, value: decoded });
}
