/**
 * Program-to-program call bindings.
 *
 * A call carries an explicitly narrowed capability list. The host refuses any
 * grant this program does not already hold and any transfer ceiling above the
 * one it was given, so authority can only shrink as a call graph deepens.
 */

import { MAX_CALL_INPUT_BYTES, MAX_CAPABILITY_ENCODING_BYTES } from "./abi";
import { CapabilitySet } from "./capability";
import { IDENTIFIER_BYTES, pointer } from "./bytes";
import {
  ERR_CAPABILITY_BYTES,
  ERR_INPUT_TOO_LARGE,
  ERR_INVALID,
  ERR_RESERVED_IDENTIFIER
} from "./error";
import { programCall } from "./host";
import { ProgramId } from "./ids";

const MINIMUM_CAPABILITY_ENCODING_BYTES: i32 = 2;

/**
 * Calls another program with an already-encoded grant list, returning the
 * callee's non-negative result code or a negative refusal.
 */
export function callProgram(
  callee: ProgramId,
  input: StaticArray<u8>,
  capabilities: StaticArray<u8>
): i32 {
  const calleeBytes = callee.bytes;
  if (calleeBytes.length != IDENTIFIER_BYTES || callee.isReserved()) return ERR_RESERVED_IDENTIFIER;
  if (input.length > MAX_CALL_INPUT_BYTES) return ERR_INPUT_TOO_LARGE;
  if (capabilities.length < MINIMUM_CAPABILITY_ENCODING_BYTES) return ERR_INVALID;
  if (capabilities.length > MAX_CAPABILITY_ENCODING_BYTES) return ERR_CAPABILITY_BYTES;
  return programCall(
    pointer(calleeBytes),
    IDENTIFIER_BYTES,
    pointer(input),
    input.length,
    pointer(capabilities),
    capabilities.length
  );
}

/**
 * Encodes a narrowed capability set and calls another program with it, returning
 * the callee's non-negative result code or a negative refusal.
 */
export function callProgramWith(
  callee: ProgramId,
  input: StaticArray<u8>,
  capabilities: CapabilitySet
): i32 {
  const encoded = new StaticArray<u8>(capabilities.encodedLength());
  const written = capabilities.encode(encoded);
  if (written < 0) return written;
  return callProgram(callee, input, encoded);
}
