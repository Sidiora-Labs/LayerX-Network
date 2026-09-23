/** Calldata builders, wallet send helpers and event decoders for the LayerXBridge precompile. */

import type { Eip1193Requester } from "./account-derivation.js";
import {
  decodeEventFrom,
  encodeAbiCall,
  sendPrecompileCall,
  type DecodedPrecompileEvent,
  type PrecompileCall,
  type PrecompileEventSpec,
  type PrecompileLog,
} from "./exchange.js";

export const LAYERX_BRIDGE_PRECOMPILE = "0x0000000000000000000000000000000000001016";

export const BRIDGE_EVENTS: readonly PrecompileEventSpec[] = [
  {
    name: "BridgeIn",
    precompile: LAYERX_BRIDGE_PRECOMPILE,
    inputs: [
      { name: "chain", type: "uint64", indexed: true },
      { name: "txHash", type: "bytes32", indexed: true },
      { name: "recipient", type: "address", indexed: true },
      { name: "logIndex", type: "uint64", indexed: false },
      { name: "asset", type: "address", indexed: false },
      { name: "amount", type: "uint256", indexed: false },
      { name: "denom", type: "string", indexed: false },
    ],
  },
  {
    name: "BridgeOut",
    precompile: LAYERX_BRIDGE_PRECOMPILE,
    inputs: [
      { name: "chain", type: "uint64", indexed: true },
      { name: "asset", type: "address", indexed: true },
      { name: "amount", type: "uint256", indexed: false },
      { name: "recipient", type: "address", indexed: false },
      { name: "nonce", type: "uint64", indexed: true },
    ],
  },
];

/** Decodes a LayerXBridge precompile log. */
export function decodeBridgeEvent(log: PrecompileLog): DecodedPrecompileEvent {
  return decodeEventFrom(BRIDGE_EVENTS, log);
}

export interface BridgeInAttestation {
  readonly chain: bigint;
  readonly vault: string;
  readonly txHash: string;
  readonly logIndex: bigint;
  readonly recipient: string;
  readonly asset: string;
  readonly amount: bigint;
  readonly signatures: readonly string[];
}

/** `bridgeIn(uint64,address,bytes32,uint64,bytes32,address,uint256,bytes[])`. */
export function bridgeInCall(attestation: BridgeInAttestation): PrecompileCall {
  return {
    to: LAYERX_BRIDGE_PRECOMPILE,
    data: encodeAbiCall(
      "bridgeIn",
      ["uint64", "address", "bytes32", "uint64", "bytes32", "address", "uint256", "bytes[]"],
      [
        attestation.chain,
        attestation.vault,
        attestation.txHash,
        attestation.logIndex,
        attestation.recipient,
        attestation.asset,
        attestation.amount,
        attestation.signatures,
      ],
    ),
    value: 0n,
  };
}

/** `bridgeOut(uint64,address,uint256,address)`. */
export function bridgeOutCall(chain: bigint, asset: string, amount: bigint, recipient: string): PrecompileCall {
  return {
    to: LAYERX_BRIDGE_PRECOMPILE,
    data: encodeAbiCall("bridgeOut", ["uint64", "address", "uint256", "address"], [chain, asset, amount, recipient]),
    value: 0n,
  };
}

export const sendBridgeIn = (wallet: Eip1193Requester, from: string, attestation: BridgeInAttestation): Promise<string> =>
  sendPrecompileCall(wallet, from, bridgeInCall(attestation));

export const sendBridgeOut = (
  wallet: Eip1193Requester,
  from: string,
  chain: bigint,
  asset: string,
  amount: bigint,
  recipient: string,
): Promise<string> => sendPrecompileCall(wallet, from, bridgeOutCall(chain, asset, amount, recipient));
