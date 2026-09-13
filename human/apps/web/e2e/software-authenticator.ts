import { createHash, generateKeyPairSync, randomBytes, sign, type KeyObject } from "node:crypto";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

type Cbor = string | number | Uint8Array | ReadonlyMap<string | number, Cbor>;

function cborHead(major: number, value: number): Buffer {
  if (!Number.isSafeInteger(value) || value < 0 || value > 65_535) throw new Error("CBOR bound");
  if (value < 24) return Buffer.from([(major << 5) | value]);
  if (value <= 255) return Buffer.from([(major << 5) | 24, value]);
  return Buffer.from([(major << 5) | 25, value >> 8, value & 255]);
}

function cbor(value: Cbor): Buffer {
  if (typeof value === "number") return cborHead(value < 0 ? 1 : 0, value < 0 ? -1 - value : value);
  if (typeof value === "string") {
    const bytes = Buffer.from(value, "utf8");
    return Buffer.concat([cborHead(3, bytes.length), bytes]);
  }
  if (value instanceof Uint8Array) return Buffer.concat([cborHead(2, value.length), value]);
  const entries = [...value.entries()].map(([key, item]) => [cbor(key), cbor(item)] as const)
    .sort(([left], [right]) => left.length - right.length || Buffer.compare(left, right));
  return Buffer.concat([cborHead(5, entries.length), ...entries.flat()]);
}

function record(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("Object required");
  return value as Record<string, unknown>;
}

function text(value: unknown): string {
  if (typeof value !== "string" || value.length === 0 || value.length > 16_384) throw new Error("Text bound");
  return value;
}

function options(ceremony: string): Record<string, unknown> {
  if (!/^[A-Za-z0-9_-]+$/u.test(text(ceremony))) throw new Error("Ceremony encoding");
  return record(JSON.parse(Buffer.from(ceremony, "base64url").toString("utf8")) as unknown);
}

function encoded(value: unknown): string {
  return Buffer.from(JSON.stringify(value), "utf8").toString("base64url");
}

export class SoftwareAuthenticator {
  readonly #origin: string;
  readonly #rpId: string;
  readonly #key: KeyObject;
  readonly #publicKey: Buffer;
  readonly #credentialId = randomBytes(32);
  #counter = 0;
  #userHandle: string | undefined;

  constructor(origin: string) {
    const endpoint = new URL(origin);
    if (endpoint.protocol !== "https:" || endpoint.origin !== origin || endpoint.port !== ""
      || endpoint.username !== "" || endpoint.password !== "") throw new Error("HTTPS origin required");
    this.#origin = origin;
    this.#rpId = endpoint.hostname;
    const pair = generateKeyPairSync("ed25519");
    this.#key = pair.privateKey;
    this.#publicKey = Buffer.from(text(pair.publicKey.export({ format: "jwk" }).x), "base64url");
    if (this.#publicKey.length !== 32) throw new Error("Public key length");
  }

  #clientData(kind: string, challenge: unknown): Buffer {
    return Buffer.from(JSON.stringify({ type: kind, challenge: text(challenge),
      origin: this.#origin, crossOrigin: false }), "utf8");
  }

  #authenticatorData(attested: boolean): Buffer {
    const counter = Buffer.alloc(4);
    counter.writeUInt32BE(this.#counter);
    const prefix = [createHash("sha256").update(this.#rpId).digest(),
      Buffer.from([attested ? 0x45 : 0x05]), counter];
    if (!attested) return Buffer.concat(prefix);
    const credentialLength = Buffer.alloc(2);
    credentialLength.writeUInt16BE(this.#credentialId.length);
    const key = cbor(new Map<string | number, Cbor>([[1, 1], [3, -8], [-1, 6], [-2, this.#publicKey]]));
    return Buffer.concat([...prefix, Buffer.alloc(16), credentialLength, this.#credentialId, key]);
  }

  register(ceremony: string): string {
    const value = options(ceremony);
    if (text(record(value.rp).id) !== this.#rpId) throw new Error("Relying party mismatch");
    const parameters = value.pubKeyCredParams;
    if (!Array.isArray(parameters) || !parameters.some((item: unknown) => record(item).alg === -8)) {
      throw new Error("Ed25519 is not offered");
    }
    this.#userHandle = text(record(value.user).id);
    const attestation = cbor(new Map<string | number, Cbor>([
      ["fmt", "none"], ["attStmt", new Map()], ["authData", this.#authenticatorData(true)],
    ]));
    return encoded({ id: this.#credentialId.toString("base64url"), transports: ["internal"],
      attestationObject: attestation.toString("base64url"),
      clientDataJSON: this.#clientData("webauthn.create", value.challenge).toString("base64url") });
  }

  assert(ceremony: string): string {
    const value = options(ceremony);
    if (text(value.rpId) !== this.#rpId || this.#userHandle === undefined) throw new Error("Unbound assertion");
    if (this.#counter === 0xffff_ffff) throw new Error("Authenticator counter exhausted");
    this.#counter += 1;
    const authenticatorData = this.#authenticatorData(false);
    const clientData = this.#clientData("webauthn.get", value.challenge);
    const signed = Buffer.concat([authenticatorData, createHash("sha256").update(clientData).digest()]);
    return encoded({ id: this.#credentialId.toString("base64url"),
      authenticatorData: authenticatorData.toString("base64url"),
      signature: sign(null, signed, this.#key).toString("base64url"),
      clientDataJSON: clientData.toString("base64url"), userHandle: this.#userHandle });
  }
}

async function main(): Promise<void> {
  const authenticator = new SoftwareAuthenticator(text(process.argv[2]));
  const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
  for await (const line of lines) {
    const request = record(JSON.parse(text(line)) as unknown);
    const ceremony = text(request.ceremony);
    const credential = request.operation === "register" ? authenticator.register(ceremony)
      : request.operation === "assert" ? authenticator.assert(ceremony)
        : (() => { throw new Error("Unknown authenticator operation"); })();
    process.stdout.write(`${JSON.stringify({ credential })}\n`);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  void main().catch(() => { process.stderr.write("Authenticator request refused\n"); process.exitCode = 1; });
}
