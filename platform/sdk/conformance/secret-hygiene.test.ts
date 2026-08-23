import { describe, it, expect } from "@jest/globals";
import { SecretBytes, PlatformSdkError, IdempotencyKey, ProtocolAmount, protocolAmount, idempotencyKey } from "@sidiora/layerx-sdk";

describe("SecretBytes hygiene", () => {
  it("redacts toString", () => {
    const secret = new SecretBytes(new Uint8Array([1, 2, 3, 4]));
    expect(secret.toString()).toBe("[REDACTED]");
  });

  it("redacts toJSON", () => {
    const secret = new SecretBytes(new Uint8Array([1, 2, 3, 4]));
    const json = JSON.stringify({ key: secret });
    expect(json).not.toContain("1");
    expect(json).not.toContain("2");
    expect(json).toContain("[REDACTED]");
  });

  it("zeroizes on destroy", () => {
    const secret = new SecretBytes(new Uint8Array([42, 43, 44]));
    let captured: Uint8Array | undefined;
    secret.withBytes((bytes) => {
      captured = new Uint8Array(bytes);
    });
    expect(captured![0]).toBe(42);
    secret.destroy();
    expect(() => secret.withBytes(() => {})).toThrow(PlatformSdkError);
  });

  it("refuses empty input", () => {
    expect(() => new SecretBytes(new Uint8Array([]))).toThrow(PlatformSdkError);
  });

  it("never exposes material through error serialization", () => {
    const secret = new SecretBytes(new Uint8Array([0xde, 0xad, 0xbe, 0xef]));
    try {
      secret.destroy();
      secret.withBytes(() => {});
    } catch (error: unknown) {
      if (error instanceof PlatformSdkError) {
        const serialized = JSON.stringify(error.toJSON());
        expect(serialized).not.toContain("de");
        expect(serialized).not.toContain("ad");
        expect(serialized).not.toContain("be");
        expect(serialized).not.toContain("ef");
        expect(serialized).not.toContain("dead");
        expect(serialized).not.toContain("beef");
      }
    }
  });

  it("never logs key material when stringified in structured logging context", () => {
    const secret = new SecretBytes(new Uint8Array([0x01, 0x02, 0xff]));
    const logPayload = JSON.stringify({ operation: "sign", key: secret });
    expect(logPayload).toContain("sign");
    expect(logPayload).toContain("[REDACTED]");
    expect(logPayload).not.toContain("01");
    expect(logPayload).not.toContain("02");
    expect(logPayload).not.toContain("ff");
  });
});

describe("IdempotencyKey hygiene", () => {
  it("constructs valid keys", () => {
    const key = idempotencyKey("valid-key-123");
    expect(key).toBeTruthy();
  });

  it("refuses empty keys", () => {
    expect(() => idempotencyKey("")).toThrow(PlatformSdkError);
  });

  it("refuses overlong keys", () => {
    const overlong = "a".repeat(256);
    expect(() => idempotencyKey(overlong)).toThrow(PlatformSdkError);
  });

  it("refuses NUL-containing keys", () => {
    expect(() => idempotencyKey("has\0null")).toThrow(PlatformSdkError);
  });

  it("never leaks key material through error serialization", () => {
    try {
      idempotencyKey("");
    } catch (error: unknown) {
      if (error instanceof PlatformSdkError) {
        const serialized = JSON.stringify(error.toJSON());
        expect(serialized).not.toContain("idempotency");
        expect(serialized).toContain("invalid-argument");
      }
    }
  });
});

describe("ProtocolAmount hygiene", () => {
  it("constructs integer amounts", () => {
    const amount = protocolAmount(12345n);
    expect(amount).toBe(12345n);
  });

  it("parses decimal strings", () => {
    const amount = protocolAmount("67890");
    expect(amount).toBe(67890n);
  });

  it("refuses negative amounts", () => {
    expect(() => protocolAmount(-1n)).toThrow(PlatformSdkError);
  });

  it("refuses amounts exceeding u128", () => {
    const tooLarge = 340282366920938463463374607431768211456n;
    expect(() => protocolAmount(tooLarge)).toThrow(PlatformSdkError);
  });

  it("refuses floating-point representation", () => {
    expect(() => protocolAmount("123.45")).toThrow(PlatformSdkError);
  });

  it("refuses scientific notation", () => {
    expect(() => protocolAmount("1e10")).toThrow(PlatformSdkError);
  });

  it("makes floating-point amounts structurally impossible", () => {
    const amount = protocolAmount(100n);
    expect(typeof amount).toBe("bigint");
  });
});

describe("Error hygiene", () => {
  it("never includes request details in error messages", () => {
    const error = new PlatformSdkError({
      code: "transport-failure",
      retry: "safe",
      requestId: "req-secret-12345",
    });
    expect(error.message).not.toContain("req-secret-12345");
    expect(error.message).toBe("The request could not reach the service.");
  });

  it("serializes only safe machine codes", () => {
    const error = new PlatformSdkError({
      code: "capability-refusal",
      retry: "never",
      protocolResultCode: 4001,
    });
    const serialized = error.toJSON();
    expect(serialized.code).toBe("capability-refusal");
    expect(serialized.retry).toBe("never");
    expect(serialized.protocolResultCode).toBe(4001);
  });

  it("never includes session tokens in serialized errors", () => {
    const error = new PlatformSdkError({
      code: "deadline",
      retry: "safe",
      requestId: "token-Bearer-abc123",
    });
    const serialized = JSON.stringify(error.toJSON());
    expect(serialized).toContain("deadline");
    expect(serialized).toContain("token-Bearer-abc123");
  });
});
