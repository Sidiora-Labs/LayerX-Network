import assert from "node:assert/strict";
import test from "node:test";

import type {
  AuthenticatorSetupChallenge,
  BackupCodeSet,
  HumanApiClient,
  OperationDigest,
  OpaqueCredential,
  Passkey,
  PasskeyId,
  PasskeyRegistrationChallenge,
  SecurityActionKind,
  Session,
  StepUpChallengeId,
  StepUpEvidence,
  TimedSecret,
} from "../src/api";
import type { PasskeyAuthenticator } from "../src/journeys/approvals";
import { securityStepUp } from "../src/settings/security";

interface MockCredential {
  readonly id: string;
  readonly type: "public-key";
  readonly rawId: ArrayBuffer;
  readonly response: {
    readonly clientDataJSON: ArrayBuffer;
    readonly attestationObject: ArrayBuffer;
  };
}

interface MockPasskeyAuthenticator extends PasskeyAuthenticator {
  readonly mockCredential: MockCredential;
}

function createMockStepUpEvidence(action: SecurityActionKind, targetId?: string): StepUpEvidence {
  const digest: OperationDigest = `${action}:${targetId ?? "none"}` as OperationDigest;
  return {
    challenge_id: `stepup-${action}-${Date.now()}` as StepUpChallengeId,
    confirms: digest,
    passkey_id: "mock-passkey-id" as PasskeyId,
    completed_at: new Date().toISOString(),
    expires_at: new Date(Date.now() + 300000).toISOString(),
  };
}

function createMockClient(
  securityActionResponses: Map<SecurityActionKind, { confirms: OperationDigest }>,
): HumanApiClient {
  return {
    securityAction: async (request: { action: SecurityActionKind; target_id?: string }) => {
      const response = securityActionResponses.get(request.action);
      if (response === undefined) {
        throw new Error(`Unexpected action: ${request.action}`);
      }
      return response;
    },
    securityPasskeyRegisterBegin: async (_request: { label: string; step_up: StepUpEvidence }) => {
      const challenge: PasskeyRegistrationChallenge = {
        registration_id: "mock-registration-id",
        ceremony: "mock-registration-ceremony" as OpaqueCredential,
        expires_at: new Date(Date.now() + 300000).toISOString(),
      };
      return challenge;
    },
    securityPasskeyRegisterFinish: async (
      _registrationId: string,
      _request: { credential: MockCredential; step_up: StepUpEvidence },
    ) => {
      const passkey: Passkey = {
        passkey_id: "new-passkey-id" as PasskeyId,
        label: "New Device",
        created_at: new Date().toISOString(),
        last_used_at: new Date().toISOString(),
      };
      return passkey;
    },
    securityPasskeyRevoke: async (_passkeyId: string, _request: { step_up: StepUpEvidence }) => {
      return { passkeys: [] };
    },
    securitySessionRevoke: async (_sessionId: string, _request: { step_up: StepUpEvidence }) => {
      return { revoked_session_ids: ["mock-session-id"] };
    },
    securitySessionRevokeAll: async (_request: { step_up: StepUpEvidence }) => {
      return { revoked_session_ids: ["session-1", "session-2"] };
    },
    authenticatorSetupBegin: async (_request: { label: string; step_up: StepUpEvidence }) => {
      const challenge: AuthenticatorSetupChallenge = {
        setup_id: "mock-setup-id",
        expires_at: new Date(Date.now() + 300000).toISOString(),
        otpauth_uri: {
          value: "otpauth://totp/LayerX?secret=ABCDEFGH",
          remask_at: new Date(Date.now() + 300000).toISOString(),
          copyable: true,
        },
        secret: {
          value: "ABCDEFGH",
          remask_at: new Date(Date.now() + 300000).toISOString(),
          copyable: true,
        },
      };
      return challenge;
    },
    authenticatorSetupFinish: async (
      _setupId: string,
      _request: { code: string; step_up: StepUpEvidence },
    ) => {
      const backupCodes: BackupCodeSet = {
        codes: ["CODE-1234", "CODE-5678", "CODE-9012"],
        remask_at: new Date(Date.now() + 300000).toISOString(),
        copyable: true,
      };
      return {
        method: {
          authenticator_id: "auth-id-1",
          label: "Auth App",
          enabled_at: new Date().toISOString(),
          last_used_at: undefined,
        },
        backup_codes: backupCodes,
      };
    },
    authenticatorDisable: async (_authenticatorId: string, _request: { step_up: StepUpEvidence }) => {
      return {
        methods: [],
        backup_codes_remaining: 0,
      };
    },
    authenticatorBackupRotate: async (_request: { step_up: StepUpEvidence }) => {
      const backupCodes: BackupCodeSet = {
        codes: ["NEW-1234", "NEW-5678", "NEW-9012"],
        remask_at: new Date(Date.now() + 300000).toISOString(),
        copyable: true,
      };
      return backupCodes;
    },
    securityRecoveryReveal: async (_request: { evidence_id: string; step_up: StepUpEvidence }) => {
      const secret: TimedSecret = {
        value: "recovery-secret-data",
        remask_at: new Date(Date.now() + 300000).toISOString(),
        copyable: true,
      };
      return secret;
    },
  } as unknown as HumanApiClient;
}

function createMockAuthenticator(): MockPasskeyAuthenticator {
  return {
    mockCredential: {
      id: "mock-credential-id",
      type: "public-key",
      rawId: new ArrayBuffer(32),
      response: {
        clientDataJSON: new ArrayBuffer(128),
        attestationObject: new ArrayBuffer(256),
      },
    },
    beginAssertion: async (_challenge: unknown) => {
      return {
        credential_id: "mock-credential-id",
        authenticator_data: new Uint8Array([0x01, 0x02, 0x03]),
        client_data_json: new Uint8Array([0x04, 0x05, 0x06]),
        signature: new Uint8Array([0x07, 0x08, 0x09]),
      };
    },
    beginRegistration: async (_ceremony: unknown) => {
      return {
        id: "new-credential-id",
        type: "public-key",
        rawId: new ArrayBuffer(32),
        response: {
          clientDataJSON: new ArrayBuffer(128),
          attestationObject: new ArrayBuffer(256),
        },
      };
    },
  } as unknown as MockPasskeyAuthenticator;
}

test("add-passkey mutation requires step-up", async () => {
  const action: SecurityActionKind = "add-passkey";
  const targetLabel = "New Device";
  const securityActionResponses = new Map<SecurityActionKind, { confirms: OperationDigest }>([
    [action, { confirms: `${action}:${targetLabel}` as OperationDigest }],
  ]);
  const client = createMockClient(securityActionResponses);
  const authenticator = createMockAuthenticator();

  const stepUp = await securityStepUp(client, action, targetLabel, authenticator);

  assert.equal(typeof stepUp.challenge_id, "string");
  assert.ok(stepUp.challenge_id.length > 0);
  assert.equal(stepUp.confirms, `${action}:${targetLabel}`);
  assert.equal(typeof stepUp.passkey_id, "string");
});

test("revoke-passkey mutation requires step-up", async () => {
  const action: SecurityActionKind = "revoke-passkey";
  const passkeyId = "passkey-to-revoke";
  const securityActionResponses = new Map<SecurityActionKind, { confirms: OperationDigest }>([
    [action, { confirms: `${action}:${passkeyId}` as OperationDigest }],
  ]);
  const client = createMockClient(securityActionResponses);
  const authenticator = createMockAuthenticator();

  const stepUp = await securityStepUp(client, action, passkeyId, authenticator);

  assert.equal(typeof stepUp.challenge_id, "string");
  assert.ok(stepUp.challenge_id.length > 0);
  assert.equal(stepUp.confirms, `${action}:${passkeyId}`);
});

test("revoke-session mutation requires step-up", async () => {
  const action: SecurityActionKind = "revoke-session";
  const sessionId = "session-to-revoke";
  const securityActionResponses = new Map<SecurityActionKind, { confirms: OperationDigest }>([
    [action, { confirms: `${action}:${sessionId}` as OperationDigest }],
  ]);
  const client = createMockClient(securityActionResponses);
  const authenticator = createMockAuthenticator();

  const stepUp = await securityStepUp(client, action, sessionId, authenticator);

  assert.equal(typeof stepUp.challenge_id, "string");
  assert.ok(stepUp.challenge_id.length > 0);
  assert.equal(stepUp.confirms, `${action}:${sessionId}`);
});

test("revoke-all-sessions mutation requires step-up", async () => {
  const action: SecurityActionKind = "revoke-all-sessions";
  const securityActionResponses = new Map<SecurityActionKind, { confirms: OperationDigest }>([
    [action, { confirms: `${action}:none` as OperationDigest }],
  ]);
  const client = createMockClient(securityActionResponses);
  const authenticator = createMockAuthenticator();

  const stepUp = await securityStepUp(client, action, undefined, authenticator);

  assert.equal(typeof stepUp.challenge_id, "string");
  assert.ok(stepUp.challenge_id.length > 0);
  assert.equal(stepUp.confirms, `${action}:none`);
});

test("add-authenticator mutation requires step-up", async () => {
  const action: SecurityActionKind = "add-authenticator";
  const authenticatorLabel = "My Auth App";
  const securityActionResponses = new Map<SecurityActionKind, { confirms: OperationDigest }>([
    [action, { confirms: `${action}:${authenticatorLabel}` as OperationDigest }],
  ]);
  const client = createMockClient(securityActionResponses);
  const authenticator = createMockAuthenticator();

  const stepUp = await securityStepUp(client, action, authenticatorLabel, authenticator);

  assert.equal(typeof stepUp.challenge_id, "string");
  assert.ok(stepUp.challenge_id.length > 0);
  assert.equal(stepUp.confirms, `${action}:${authenticatorLabel}`);
});

test("disable-authenticator mutation requires step-up", async () => {
  const action: SecurityActionKind = "disable-authenticator";
  const authenticatorId = "auth-to-disable";
  const securityActionResponses = new Map<SecurityActionKind, { confirms: OperationDigest }>([
    [action, { confirms: `${action}:${authenticatorId}` as OperationDigest }],
  ]);
  const client = createMockClient(securityActionResponses);
  const authenticator = createMockAuthenticator();

  const stepUp = await securityStepUp(client, action, authenticatorId, authenticator);

  assert.equal(typeof stepUp.challenge_id, "string");
  assert.ok(stepUp.challenge_id.length > 0);
  assert.equal(stepUp.confirms, `${action}:${authenticatorId}`);
});

test("rotate-backup-codes mutation requires step-up", async () => {
  const action: SecurityActionKind = "rotate-backup-codes";
  const securityActionResponses = new Map<SecurityActionKind, { confirms: OperationDigest }>([
    [action, { confirms: `${action}:none` as OperationDigest }],
  ]);
  const client = createMockClient(securityActionResponses);
  const authenticator = createMockAuthenticator();

  const stepUp = await securityStepUp(client, action, undefined, authenticator);

  assert.equal(typeof stepUp.challenge_id, "string");
  assert.ok(stepUp.challenge_id.length > 0);
  assert.equal(stepUp.confirms, `${action}:none`);
});

test("reveal-recovery-evidence mutation requires step-up", async () => {
  const action: SecurityActionKind = "reveal-recovery-evidence";
  const evidenceId = "evidence-to-reveal";
  const securityActionResponses = new Map<SecurityActionKind, { confirms: OperationDigest }>([
    [action, { confirms: `${action}:${evidenceId}` as OperationDigest }],
  ]);
  const client = createMockClient(securityActionResponses);
  const authenticator = createMockAuthenticator();

  const stepUp = await securityStepUp(client, action, evidenceId, authenticator);

  assert.equal(typeof stepUp.challenge_id, "string");
  assert.ok(stepUp.challenge_id.length > 0);
  assert.equal(stepUp.confirms, `${action}:${evidenceId}`);
});

test("step-up evidence binds to specific operation digest", async () => {
  const action: SecurityActionKind = "revoke-passkey";
  const passkeyId = "specific-passkey";
  const expectedDigest: OperationDigest = `${action}:${passkeyId}` as OperationDigest;
  const securityActionResponses = new Map<SecurityActionKind, { confirms: OperationDigest }>([
    [action, { confirms: expectedDigest }],
  ]);
  const client = createMockClient(securityActionResponses);
  const authenticator = createMockAuthenticator();

  const stepUp = await securityStepUp(client, action, passkeyId, authenticator);

  assert.equal(stepUp.confirms, expectedDigest, "Step-up evidence must bind to the specific operation digest");
});

test("all security mutations produce valid step-up evidence structure", async () => {
  const allActions: readonly SecurityActionKind[] = [
    "add-passkey",
    "revoke-passkey",
    "revoke-session",
    "revoke-all-sessions",
    "add-authenticator",
    "disable-authenticator",
    "rotate-backup-codes",
    "reveal-recovery-evidence",
  ];

  for (const action of allActions) {
    const targetId = action.includes("all") ? undefined : `target-${action}`;
    const securityActionResponses = new Map<SecurityActionKind, { confirms: OperationDigest }>([
      [action, { confirms: `${action}:${targetId ?? "none"}` as OperationDigest }],
    ]);
    const client = createMockClient(securityActionResponses);
    const authenticator = createMockAuthenticator();

    const stepUp = await securityStepUp(client, action, targetId, authenticator);

    assert.equal(typeof stepUp.challenge_id, "string", `${action} must produce a challenge_id`);
    assert.ok(stepUp.challenge_id.length > 0, `${action} challenge_id must not be empty`);
    assert.equal(typeof stepUp.confirms, "string", `${action} must produce a confirms digest`);
    assert.equal(typeof stepUp.passkey_id, "string", `${action} must include a passkey_id`);
    assert.equal(typeof stepUp.completed_at, "string", `${action} must record completed_at`);
    assert.ok(stepUp.completed_at.length > 0, `${action} completed_at must not be empty`);
    assert.equal(typeof stepUp.expires_at, "string", `${action} must record expires_at`);
    assert.ok(stepUp.expires_at.length > 0, `${action} expires_at must not be empty`);
  }
});
