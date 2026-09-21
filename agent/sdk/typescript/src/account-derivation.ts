/**
 * One secret, one account on both sides of the Paxeer X Network.
 *
 * A Paxeer EVM account is a secp256k1 key and a LayerX identity is an Ed25519
 * key. Both come from one user secret: a BIP-39 mnemonic (BIP-32 at
 * m/44'/60'/0'/0/i and SLIP-0010 Ed25519 at m/44'/19544'/i'/0'), or, for a
 * wallet that never reveals its seed, one fixed EIP-712 signature fed to
 * HKDF-SHA256. This module imports nothing from Node and runs in a browser.
 */
import { ed25519 } from "@noble/curves/ed25519.js";
import { secp256k1 } from "@noble/curves/secp256k1.js";
import { hkdf } from "@noble/hashes/hkdf.js";
import { hmac } from "@noble/hashes/hmac.js";
import { sha256, sha512 } from "@noble/hashes/sha2.js";
import { keccak_256 } from "@noble/hashes/sha3.js";
import { HDKey } from "@scure/bip32";
import { mnemonicToSeedSync, validateMnemonic } from "@scure/bip39";
import { wordlist } from "@scure/bip39/wordlists/english.js";

/** SLIP-0044 style coin type of the LayerX branch: 0x4c58, ASCII "LX". */
export const LAYERX_COIN_TYPE = 19544;
export const ACCOUNT_DERIVATION_ORIGIN = "https://app.layerx.network";
export const ACCOUNT_DERIVATION_DOMAIN_NAME = "Paxeer X Network";
export const ACCOUNT_DERIVATION_DOMAIN_VERSION = "1";
export const ACCOUNT_DERIVATION_PURPOSE = "Derive your LayerX account key";
export const ACCOUNT_DERIVATION_VERSION = 1;
export const ACCOUNT_DERIVATION_HKDF_SALT = "paxeer-x-network/layerx-account-key/v1";
export const ACCOUNT_DERIVATION_HKDF_INFO_PREFIX = "LX:ACCOUNT-KEY:v1";
export const ADDR_PRECOMPILE_ADDRESS = "0x0000000000000000000000000000000000001004";

const HARDENED = 0x80000000;
const SECP256K1_ORDER = 0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141n;
const ORIGIN = /^https?:\/\/[a-z0-9.-]{1,253}(:[0-9]{1,5})?$/u;
const ADDRESS = /^0x[0-9a-fA-F]{40}$/u;
const DOMAIN_TYPE = "EIP712Domain(string name,string version,uint256 chainId)";
const MESSAGE_TYPE =
  "LayerXKeyDerivation(string purpose,string warning,address address,uint32 index,uint32 version)";
const SELECTOR_BIND_LAYERX = "dd9aa628";
const SELECTOR_LAYERX_BIND_NONCE = "cedd9ba2";
const SELECTOR_GET_UNIFIED_ACCOUNT = "357feed6";
const BIND_DOMAIN = "LX:PAXEER-BIND:v1";

export type AccountDerivationErrorCode =
  | "invalid_mnemonic"
  | "invalid_seed"
  | "index_out_of_range"
  | "invalid_chain_id"
  | "invalid_address"
  | "invalid_origin"
  | "invalid_wallet_signature"
  | "wallet_signer_mismatch"
  | "wallet_signature_not_deterministic"
  | "bound_to_different_did"
  | "malformed_precompile_answer"
  | "evm_key_unavailable";

export class AccountDerivationError extends Error {
  public constructor(
    public readonly code: AccountDerivationErrorCode,
    detail?: string,
  ) {
    super(detail === undefined ? code : `${code}: ${detail}`);
    this.name = "AccountDerivationError";
  }
}

/** The pair one secret yields. `evmPrivateKey` is null when a wallet holds it. */
export interface DerivedAccount {
  readonly index: number;
  readonly evmAddress: string;
  readonly evmPrivateKey: Uint8Array | null;
  readonly layerxSeed: Uint8Array;
  readonly layerxPublicKey: string;
  readonly did: string;
}

function toHex(bytes: Uint8Array): string {
  let text = "";
  for (const byte of bytes) {
    text += byte.toString(16).padStart(2, "0");
  }
  return text;
}

function fromHex(text: string, code: AccountDerivationErrorCode): Uint8Array {
  const body = text.startsWith("0x") || text.startsWith("0X") ? text.slice(2) : text;
  if (body.length % 2 !== 0 || !/^[0-9a-fA-F]*$/u.test(body)) {
    throw new AccountDerivationError(code);
  }
  const bytes = new Uint8Array(body.length / 2);
  for (let at = 0; at < bytes.length; at += 1) {
    bytes[at] = Number.parseInt(body.slice(at * 2, at * 2 + 2), 16);
  }
  return bytes;
}

function concat(...parts: readonly Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((total, part) => total + part.length, 0));
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

function utf8(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}

function word(value: bigint): Uint8Array {
  return fromHex(value.toString(16).padStart(64, "0"), "invalid_chain_id");
}

function u32(value: number): Uint8Array {
  return fromHex(value.toString(16).padStart(8, "0"), "index_out_of_range");
}

function checkIndex(index: number): number {
  if (!Number.isInteger(index) || index < 0 || index >= HARDENED) {
    throw new AccountDerivationError("index_out_of_range");
  }
  return index;
}

function checkChainId(chainId: number | bigint): bigint {
  const value = typeof chainId === "bigint" ? chainId : Number.isInteger(chainId) ? BigInt(chainId) : -1n;
  if (value < 0n || value > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new AccountDerivationError("invalid_chain_id");
  }
  return value;
}

function addressBytes(address: string): Uint8Array {
  if (!ADDRESS.test(address)) {
    throw new AccountDerivationError("invalid_address");
  }
  return fromHex(address, "invalid_address");
}

/** Renders a 20-byte address with its EIP-55 checksum. */
export function checksumAddress(address: Uint8Array | string): string {
  const lower = typeof address === "string" ? toHex(addressBytes(address)) : toHex(address);
  const digest = toHex(keccak_256(utf8(lower)));
  let text = "0x";
  for (let at = 0; at < lower.length; at += 1) {
    const character = lower.charAt(at);
    text += Number.parseInt(digest.charAt(at), 16) >= 8 ? character.toUpperCase() : character;
  }
  return text;
}

function evmAddressOf(uncompressedPublicKey: Uint8Array): string {
  return checksumAddress(keccak_256(uncompressedPublicKey.slice(1)).slice(12));
}

function account(
  index: number,
  evmAddress: string,
  evmPrivateKey: Uint8Array | null,
  layerxSeed: Uint8Array,
): DerivedAccount {
  const layerxPublicKey = toHex(ed25519.getPublicKey(layerxSeed));
  return { index, evmAddress, evmPrivateKey, layerxSeed, layerxPublicKey, did: `did:layerx:${layerxPublicKey}` };
}

/** SLIP-0010 Ed25519 private key at a hardened-only path; components carry no hardening bit. */
export function slip10Ed25519(seed: Uint8Array, path: readonly number[]): Uint8Array {
  if (seed.length < 16 || seed.length > 64) {
    throw new AccountDerivationError("invalid_seed");
  }
  let node = hmac(sha512, utf8("ed25519 seed"), seed);
  for (const component of path) {
    const index = u32(checkIndex(component) + HARDENED);
    node = hmac(sha512, node.slice(32), concat(new Uint8Array([0]), node.slice(0, 32), index));
  }
  return node.slice(0, 32);
}

/** Derives both keys of account `index` from a BIP-32 seed. */
export function deriveFromSeed(seed: Uint8Array, index = 0): DerivedAccount {
  checkIndex(index);
  if (seed.length < 16 || seed.length > 64) {
    throw new AccountDerivationError("invalid_seed");
  }
  const evm = HDKey.fromMasterSeed(seed).derive(`m/44'/60'/0'/0/${index}`).privateKey;
  if (evm === null) {
    throw new AccountDerivationError("invalid_seed");
  }
  return account(
    index,
    evmAddressOf(secp256k1.getPublicKey(evm, false)),
    evm,
    slip10Ed25519(seed, [44, LAYERX_COIN_TYPE, index, 0]),
  );
}

/** Derives both keys of account `index` from an English BIP-39 mnemonic. */
export function deriveFromMnemonic(
  mnemonic: string,
  options: Readonly<{ passphrase?: string; index?: number }> = {},
): DerivedAccount {
  const phrase = mnemonic.normalize("NFKD").trim().split(/\s+/u).join(" ");
  if (!validateMnemonic(phrase, wordlist)) {
    throw new AccountDerivationError("invalid_mnemonic");
  }
  return deriveFromSeed(mnemonicToSeedSync(phrase, options.passphrase ?? ""), options.index ?? 0);
}

export interface KeyDerivationParameters {
  readonly chainId: number | bigint;
  readonly address: string;
  readonly index?: number;
  /** A different origin is a different message and therefore a different LayerX key. */
  readonly origin?: string;
}

export interface KeyDerivationTypedData {
  readonly types: {
    readonly EIP712Domain: readonly Readonly<{ name: string; type: string }>[];
    readonly LayerXKeyDerivation: readonly Readonly<{ name: string; type: string }>[];
  };
  readonly primaryType: "LayerXKeyDerivation";
  readonly domain: Readonly<{ name: string; version: string; chainId: number }>;
  readonly message: Readonly<{ purpose: string; warning: string; address: string; index: number; version: number }>;
}

/** The one EIP-712 document an external wallet signs to derive a LayerX key. */
export function keyDerivationTypedData(parameters: KeyDerivationParameters): KeyDerivationTypedData {
  const origin = parameters.origin ?? ACCOUNT_DERIVATION_ORIGIN;
  if (!ORIGIN.test(origin)) {
    throw new AccountDerivationError("invalid_origin");
  }
  return {
    types: {
      EIP712Domain: [
        { name: "name", type: "string" },
        { name: "version", type: "string" },
        { name: "chainId", type: "uint256" },
      ],
      LayerXKeyDerivation: [
        { name: "purpose", type: "string" },
        { name: "warning", type: "string" },
        { name: "address", type: "address" },
        { name: "index", type: "uint32" },
        { name: "version", type: "uint32" },
      ],
    },
    primaryType: "LayerXKeyDerivation",
    domain: {
      name: ACCOUNT_DERIVATION_DOMAIN_NAME,
      version: ACCOUNT_DERIVATION_DOMAIN_VERSION,
      chainId: Number(checkChainId(parameters.chainId)),
    },
    message: {
      purpose: ACCOUNT_DERIVATION_PURPOSE,
      warning: `Only sign this on ${origin}. Anyone holding this signature controls your LayerX account.`,
      address: `0x${toHex(addressBytes(parameters.address))}`,
      index: checkIndex(parameters.index ?? 0),
      version: ACCOUNT_DERIVATION_VERSION,
    },
  };
}

/** The EIP-712 digest of {@link keyDerivationTypedData}. */
export function keyDerivationHash(parameters: KeyDerivationParameters): Uint8Array {
  const document = keyDerivationTypedData(parameters);
  const domain = keccak_256(
    concat(
      keccak_256(utf8(DOMAIN_TYPE)),
      keccak_256(utf8(document.domain.name)),
      keccak_256(utf8(document.domain.version)),
      word(BigInt(document.domain.chainId)),
    ),
  );
  const message = keccak_256(
    concat(
      keccak_256(utf8(MESSAGE_TYPE)),
      keccak_256(utf8(document.message.purpose)),
      keccak_256(utf8(document.message.warning)),
      new Uint8Array(12),
      addressBytes(document.message.address),
      word(BigInt(document.message.index)),
      word(BigInt(document.message.version)),
    ),
  );
  return keccak_256(concat(new Uint8Array([0x19, 0x01]), domain, message));
}

function signatureBytes(signature: Uint8Array | string): Uint8Array {
  const bytes = typeof signature === "string" ? fromHex(signature, "invalid_wallet_signature") : signature;
  if (bytes.length !== 65) {
    throw new AccountDerivationError("invalid_wallet_signature");
  }
  return bytes;
}

/**
 * Brings a 65-byte r || s || v signature to its one canonical encoding: low s
 * and v of 27 or 28, so equivalent encodings derive one key.
 */
export function normalizeWalletSignature(signature: Uint8Array | string): Uint8Array {
  const bytes = signatureBytes(signature);
  const v = bytes[64];
  let parity: number;
  if (v === 0 || v === 27) {
    parity = 0;
  } else if (v === 1 || v === 28) {
    parity = 1;
  } else {
    throw new AccountDerivationError("invalid_wallet_signature");
  }
  const r = BigInt(`0x${toHex(bytes.slice(0, 32))}`);
  let s = BigInt(`0x${toHex(bytes.slice(32, 64))}`);
  if (r === 0n || s === 0n || r >= SECP256K1_ORDER || s >= SECP256K1_ORDER) {
    throw new AccountDerivationError("invalid_wallet_signature");
  }
  if (s > SECP256K1_ORDER / 2n) {
    s = SECP256K1_ORDER - s;
    parity ^= 1;
  }
  return concat(bytes.slice(0, 32), word(s), new Uint8Array([27 + parity]));
}

/**
 * Derives the LayerX key a wallet signature stands for. The signature is
 * normalised and must recover to the address the message names before it is
 * fed to HKDF-SHA256.
 */
export function deriveFromWalletSignature(
  parameters: KeyDerivationParameters,
  signature: Uint8Array | string,
): DerivedAccount {
  const canonical = normalizeWalletSignature(signature);
  const hash = keyDerivationHash(parameters);
  const address = addressBytes(parameters.address);
  let recovered: Uint8Array;
  try {
    recovered = secp256k1.Signature.fromBytes(canonical.slice(0, 64), "compact")
      .addRecoveryBit((canonical[64] ?? 27) - 27)
      .recoverPublicKey(hash)
      .toBytes(false);
  } catch {
    throw new AccountDerivationError("invalid_wallet_signature");
  }
  const signer = evmAddressOf(recovered);
  if (signer.toLowerCase() !== `0x${toHex(address)}`) {
    throw new AccountDerivationError("wallet_signer_mismatch");
  }
  const index = checkIndex(parameters.index ?? 0);
  const info = concat(
    utf8(ACCOUNT_DERIVATION_HKDF_INFO_PREFIX),
    word(checkChainId(parameters.chainId)),
    address,
    u32(index),
  );
  const seed = hkdf(sha256, canonical, utf8(ACCOUNT_DERIVATION_HKDF_SALT), info, 32);
  return account(index, signer, null, seed);
}

/** The minimal EIP-1193 surface a browser wallet exposes. */
export interface Eip1193Requester {
  request(argument: Readonly<{ method: string; params?: readonly unknown[] }>): Promise<unknown>;
}

export interface WalletRequest {
  readonly method: string;
  readonly params: readonly unknown[];
}

/** The `eth_signTypedData_v4` request object for a browser wallet. */
export function keyDerivationRequest(parameters: KeyDerivationParameters): WalletRequest {
  const document = keyDerivationTypedData(parameters);
  return { method: "eth_signTypedData_v4", params: [document.message.address, JSON.stringify(document)] };
}

/**
 * The browser-wallet flow. The signature is requested twice and the two
 * canonical forms compared: a wallet without RFC 6979 deterministic signing
 * would give a different LayerX key on every visit, so it is refused here
 * instead of silently creating an account that cannot be recovered.
 */
export async function deriveFromBrowserWallet(
  wallet: Eip1193Requester,
  parameters: KeyDerivationParameters,
): Promise<DerivedAccount> {
  const request = keyDerivationRequest(parameters);
  const canonical: Uint8Array[] = [];
  for (let attempt = 0; attempt < 2; attempt += 1) {
    const answer = await wallet.request(request);
    if (typeof answer !== "string") {
      throw new AccountDerivationError("invalid_wallet_signature");
    }
    canonical.push(normalizeWalletSignature(answer));
  }
  const [first, second] = canonical;
  if (first === undefined || second === undefined || toHex(first) !== toHex(second)) {
    throw new AccountDerivationError(
      "wallet_signature_not_deterministic",
      "this wallet signs the same message differently each time and cannot restore a LayerX key",
    );
  }
  return deriveFromWalletSignature(parameters, first);
}

/** What the `addr` precompile holds for an EVM address. */
export interface LayerXBindState {
  /** Hex public key of the bound identity, or null without a binding. */
  readonly boundDidPublicKey: string | null;
  readonly nonce: bigint;
}

export interface BindCall {
  readonly to: string;
  readonly data: string;
  readonly didPublicKey: string;
  readonly signature: string;
  readonly nonce: bigint;
}

export type BindPlan = Readonly<{ action: "already_bound" }> | Readonly<{ action: "bind"; call: BindCall }>;

function addressCall(selector: string, address: string): string {
  return `0x${selector}${"00".repeat(12)}${toHex(addressBytes(address))}`;
}

/** `eth_call` data reading `layerXBindNonce(address)`. */
export function bindNonceCall(address: string): string {
  return addressCall(SELECTOR_LAYERX_BIND_NONCE, address);
}

/** `eth_call` data reading the bound identity through `getUnifiedAccount(address)`, which never reverts. */
export function boundDidCall(address: string): string {
  return addressCall(SELECTOR_GET_UNIFIED_ACCOUNT, address);
}

export function decodeBindNonce(answer: string): bigint {
  const bytes = fromHex(answer, "malformed_precompile_answer");
  if (bytes.length !== 32 || bytes.slice(0, 24).some((byte) => byte !== 0)) {
    throw new AccountDerivationError("malformed_precompile_answer");
  }
  return BigInt(`0x${toHex(bytes)}`);
}

export function decodeBoundDid(answer: string): string | null {
  const bytes = fromHex(answer, "malformed_precompile_answer");
  if (bytes.length < 128 || bytes.length % 32 !== 0) {
    throw new AccountDerivationError("malformed_precompile_answer");
  }
  const key = toHex(bytes.slice(64, 96));
  return /^0+$/u.test(key) ? null : key;
}

/** Reads the bind state of `address` with two `eth_call`s against the `addr` precompile. */
export async function readLayerXBindState(wallet: Eip1193Requester, address: string): Promise<LayerXBindState> {
  const call = async (data: string): Promise<string> => {
    const answer = await wallet.request({
      method: "eth_call",
      params: [{ to: ADDR_PRECOMPILE_ADDRESS, data }, "latest"],
    });
    if (typeof answer !== "string") {
      throw new AccountDerivationError("malformed_precompile_answer");
    }
    return answer;
  };
  return {
    boundDidPublicKey: decodeBoundDid(await call(boundDidCall(address))),
    nonce: decodeBindNonce(await call(bindNonceCall(address))),
  };
}

/**
 * Plans first-use binding. Idempotent when the pair is already bound; refuses
 * with `bound_to_different_did`, never producing a call, when the address
 * belongs to another identity.
 */
export function planLayerXBind(
  derived: DerivedAccount,
  chainId: number | bigint,
  state: LayerXBindState,
): BindPlan {
  if (state.boundDidPublicKey !== null) {
    if (state.boundDidPublicKey.toLowerCase() === derived.layerxPublicKey) {
      return { action: "already_bound" };
    }
    throw new AccountDerivationError("bound_to_different_did", `did:layerx:${state.boundDidPublicKey.toLowerCase()}`);
  }
  if (state.nonce < 0n || state.nonce > 0xffffffffffffffffn) {
    throw new AccountDerivationError("malformed_precompile_answer");
  }
  const message = concat(
    utf8(BIND_DOMAIN),
    word(checkChainId(chainId)),
    addressBytes(derived.evmAddress),
    word(state.nonce).slice(24),
  );
  const signature = toHex(ed25519.sign(message, derived.layerxSeed));
  const offset = toHex(word(64n));
  return {
    action: "bind",
    call: {
      to: ADDR_PRECOMPILE_ADDRESS,
      data: `0x${SELECTOR_BIND_LAYERX}${derived.layerxPublicKey}${offset}${offset}${signature}`,
      didPublicKey: derived.layerxPublicKey,
      signature,
      nonce: state.nonce,
    },
  };
}

/** The `eth_sendTransaction` request a browser wallet sends for a bind call. */
export function bindTransactionRequest(derived: DerivedAccount, call: BindCall): WalletRequest {
  return {
    method: "eth_sendTransaction",
    params: [{ from: derived.evmAddress, to: call.to, data: call.data, value: "0x0" }],
  };
}

/** Reads the chain, plans, and (unless already bound) asks the wallet to send the bind call. */
export async function autoBindLayerX(
  wallet: Eip1193Requester,
  derived: DerivedAccount,
  chainId: number | bigint,
): Promise<Readonly<{ action: "already_bound" }> | Readonly<{ action: "bind"; call: BindCall; transactionHash: string }>> {
  const plan = planLayerXBind(derived, chainId, await readLayerXBindState(wallet, derived.evmAddress));
  if (plan.action === "already_bound") {
    return plan;
  }
  const hash = await wallet.request(bindTransactionRequest(derived, plan.call));
  if (typeof hash !== "string") {
    throw new AccountDerivationError("malformed_precompile_answer", "wallet returned no transaction hash");
  }
  return { action: "bind", call: plan.call, transactionHash: hash };
}

export interface BindFees {
  readonly evmNonce: bigint;
  readonly maxPriorityFeePerGas: bigint;
  readonly maxFeePerGas: bigint;
  readonly gasLimit: bigint;
}

function rlpLength(length: number, short: number): Uint8Array {
  if (length < 56) {
    return new Uint8Array([short + length]);
  }
  const size = fromHex(length.toString(16).padStart(length.toString(16).length + (length.toString(16).length % 2), "0"), "invalid_seed");
  return concat(new Uint8Array([short + 55 + size.length]), size);
}

function rlpBytes(bytes: Uint8Array): Uint8Array {
  const first = bytes[0];
  if (bytes.length === 1 && first !== undefined && first < 0x80) {
    return bytes;
  }
  return concat(rlpLength(bytes.length, 0x80), bytes);
}

function rlpInteger(value: bigint): Uint8Array {
  if (value === 0n) {
    return rlpBytes(new Uint8Array(0));
  }
  const text = value.toString(16);
  return rlpBytes(fromHex(text.padStart(text.length + (text.length % 2), "0"), "invalid_seed"));
}

function rlpList(items: readonly Uint8Array[]): Uint8Array {
  const payload = concat(...items);
  return concat(rlpLength(payload.length, 0xc0), payload);
}

/**
 * Signs the bind call as an EIP-1559 transaction with the pair's own EVM key,
 * for `eth_sendRawTransaction`. A pair derived from a wallet signature holds
 * no EVM key and is refused with `evm_key_unavailable`.
 */
export function signBindTransaction(
  derived: DerivedAccount,
  chainId: number | bigint,
  call: BindCall,
  fees: BindFees,
): string {
  if (derived.evmPrivateKey === null) {
    throw new AccountDerivationError("evm_key_unavailable");
  }
  const fields = [
    rlpInteger(checkChainId(chainId)),
    rlpInteger(fees.evmNonce),
    rlpInteger(fees.maxPriorityFeePerGas),
    rlpInteger(fees.maxFeePerGas),
    rlpInteger(fees.gasLimit),
    rlpBytes(addressBytes(call.to)),
    rlpInteger(0n),
    rlpBytes(fromHex(call.data, "malformed_precompile_answer")),
    rlpList([]),
  ];
  const hash = keccak_256(concat(new Uint8Array([2]), rlpList(fields)));
  const signature = secp256k1.sign(hash, derived.evmPrivateKey, { prehash: false, format: "recovered" });
  const recovery = signature[0] ?? 0;
  const raw = concat(
    new Uint8Array([2]),
    rlpList([
      ...fields,
      rlpInteger(BigInt(recovery)),
      rlpInteger(BigInt(`0x${toHex(signature.slice(1, 33))}`)),
      rlpInteger(BigInt(`0x${toHex(signature.slice(33, 65))}`)),
    ]),
  );
  return `0x${toHex(raw)}`;
}
