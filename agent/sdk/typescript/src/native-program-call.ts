export interface NativeProgramCall {
  readonly programId: Uint8Array;
  readonly guestAbi: 1 | 2;
  readonly entrypoint: string;
  readonly calldata: Uint8Array;
  readonly capabilities: Uint8Array;
  readonly accessDeclaration: Uint8Array;
  readonly responseCapacity: number;
  readonly resources: readonly [bigint, bigint, bigint, bigint, bigint, bigint, bigint];
}

export function encodeNativeProgramCall(call: NativeProgramCall): Uint8Array {
  if (call.programId.length !== 32 || call.programId.every(value => value === 0)
    || (call.guestAbi !== 1 && call.guestAbi !== 2) || !/^[A-Za-z0-9_.]{1,128}$/.test(call.entrypoint)
    || call.calldata.length > 1_048_576 || call.capabilities.length > 65_535
    || call.accessDeclaration.length > 1_048_576 || !Number.isInteger(call.responseCapacity)
    || call.responseCapacity < 0 || call.responseCapacity > 1_048_576
    || call.resources.length !== 7 || call.resources.some(value => value < 0n || value > 0xffff_ffff_ffff_ffffn)) {
    throw new TypeError("invalid native program call");
  }
  const entrypoint = new TextEncoder().encode(call.entrypoint);
  const size = 106 + entrypoint.length + call.calldata.length + call.capabilities.length + call.accessDeclaration.length;
  const output = new Uint8Array(size);
  const view = new DataView(output.buffer);
  output.set(call.programId); view.setUint16(32, call.guestAbi); view.setUint16(34, entrypoint.length);
  view.setUint32(36, call.calldata.length); view.setUint16(40, call.capabilities.length);
  view.setUint32(42, call.accessDeclaration.length); view.setUint32(46, call.responseCapacity);
  call.resources.forEach((value, index) => view.setBigUint64(50 + index * 8, value));
  let offset = 106;
  for (const body of [entrypoint, call.calldata, call.capabilities, call.accessDeclaration]) {
    output.set(body, offset); offset += body.length;
  }
  return output;
}

export function decodeNativeProgramCall(payload: Uint8Array): NativeProgramCall {
  if (payload.length < 106) throw new TypeError("invalid native program call");
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
  const lengths = [view.getUint16(34), view.getUint32(36), view.getUint16(40), view.getUint32(42)];
  if (106 + lengths.reduce((total, length) => total + length, 0) !== payload.length) throw new TypeError("invalid native program call");
  let offset = 106;
  const body = (length: number): Uint8Array => { const result = payload.slice(offset, offset + length); offset += length; return result; };
  const call: NativeProgramCall = {
    programId: payload.slice(0, 32), guestAbi: view.getUint16(32) as 1 | 2,
    entrypoint: new TextDecoder("utf-8", { fatal: true }).decode(body(lengths[0]!)),
    calldata: body(lengths[1]!), capabilities: body(lengths[2]!), accessDeclaration: body(lengths[3]!),
    responseCapacity: view.getUint32(46),
    resources: [view.getBigUint64(50), view.getBigUint64(58), view.getBigUint64(66), view.getBigUint64(74), view.getBigUint64(82), view.getBigUint64(90), view.getBigUint64(98)],
  };
  encodeNativeProgramCall(call);
  return call;
}
