"use client";

import { useCallback, useEffect, useMemo, useState } from "react";

import { copyEntry } from "../../../copy/catalog";
import { formatCopy } from "../../../copy/format";
import {
  humanApi,
  type AuthenticatorSetupChallenge,
  type AuthenticatorStatus,
  type BackupCodeSet,
  type HumanApiClient,
  type Journey,
  type Passkey,
  type Session,
  type StepUpEvidence,
  type TimedSecret,
} from "../../api";
import {
  DeviceSessionList,
  DesktopDetail,
  InlineNotice,
  KitButton,
  MobileDetail,
  ScreenCard,
  SettingsRow,
  SettingsSection,
  SettingsTextInput,
} from "../../kit";
import {
  browserPasskeyAuthenticator,
  type PasskeyAuthenticator,
} from "../../journeys/approvals";
import { performRegistrationCeremony } from "../../journeys/onboarding/ceremony";
import { useAuthenticatedShell } from "../../shell/app-shell";
import { errorPresentation, ErrorSurface } from "../../states/error";
import { LoadingSurface, OfflineSurface } from "../../states/surfaces";
import { formatLastActive, recoveryPresentation, securityStepUp } from "./model";
import { BackupCodesView, TimedSecretView } from "./secret";

interface SecuritySnapshot {
  readonly passkeys: readonly Passkey[];
  readonly sessions: readonly Session[];
  readonly authenticators: AuthenticatorStatus;
  readonly onboarding: Journey;
}

type LoadState =
  | Readonly<{ state: "loading" }>
  | Readonly<{ state: "offline" }>
  | Readonly<{ state: "error"; error: unknown }>
  | Readonly<{ state: "ready"; snapshot: SecuritySnapshot }>;

interface PendingAuthenticatorSetup {
  readonly label: string;
  readonly challenge: AuthenticatorSetupChallenge;
  readonly stepUp: StepUpEvidence;
}

interface SecurityScreenProps {
  readonly client?: HumanApiClient;
  readonly authenticator?: PasskeyAuthenticator;
}

function newestFirst<T extends { readonly created_at?: string; readonly enabled_at?: string }>(
  values: readonly T[],
): T[] {
  return [...values].sort((left, right) => {
    const leftTime = Date.parse(left.created_at ?? left.enabled_at ?? "");
    const rightTime = Date.parse(right.created_at ?? right.enabled_at ?? "");
    return rightTime - leftTime;
  });
}

export function SecurityScreen({ client: suppliedClient, authenticator: suppliedAuthenticator }: SecurityScreenProps) {
  const { shell } = useAuthenticatedShell();
  const client = useMemo(() => suppliedClient ?? humanApi(), [suppliedClient]);
  const authenticator = useMemo(
    () => suppliedAuthenticator ?? browserPasskeyAuthenticator(),
    [suppliedAuthenticator],
  );
  const [loadState, setLoadState] = useState<LoadState>({ state: "loading" });
  const [notice, setNotice] = useState<string>();
  const [busy, setBusy] = useState<string>();
  const [passkeyLabel, setPasskeyLabel] = useState("");
  const [authenticatorLabel, setAuthenticatorLabel] = useState("");
  const [authenticatorCode, setAuthenticatorCode] = useState("");
  const [authenticatorSetup, setAuthenticatorSetup] = useState<PendingAuthenticatorSetup>();
  const [backupCodes, setBackupCodes] = useState<BackupCodeSet>();
  const [recoverySecret, setRecoverySecret] = useState<TimedSecret>();
  const [disableCandidate, setDisableCandidate] = useState<string>();
  const [confirmSignOutEverywhere, setConfirmSignOutEverywhere] = useState(false);
  const [recoveryDetailsOpen, setRecoveryDetailsOpen] = useState(false);

  const load = useCallback(async () => {
    setLoadState({ state: "loading" });
    setNotice(undefined);
    try {
      const [passkeys, sessions, authenticators, onboarding] = await Promise.all([
        client.securityPasskeyList(),
        client.sessionList(),
        client.authenticatorStatus(),
        client.onboardingStatus(),
      ]);
      setLoadState({
        state: "ready",
        snapshot: {
          passkeys: passkeys.passkeys,
          sessions: sessions.sessions,
          authenticators,
          onboarding,
        },
      });
    } catch (error) {
      setLoadState(navigator.onLine ? { state: "error", error } : { state: "offline" });
    }
  }, [client]);

  useEffect(() => {
    void load();
  }, [load]);

  const setSnapshot = useCallback((update: (snapshot: SecuritySnapshot) => SecuritySnapshot) => {
    setLoadState((current) => current.state === "ready"
      ? { state: "ready", snapshot: update(current.snapshot) }
      : current);
  }, []);

  const runMutation = async (key: string, operation: () => Promise<void>) => {
    if (busy !== undefined) {
      return;
    }
    setBusy(key);
    setNotice(copyEntry("security.confirming").message);
    try {
      await operation();
    } catch {
      setNotice(copyEntry("security.mutation.failed").message);
    } finally {
      setBusy(undefined);
    }
  };

  const addPasskey = async () => {
    const label = passkeyLabel.trim();
    await runMutation("add-passkey", async () => {
      const stepUp = await securityStepUp(client, "add-passkey", label, authenticator);
      const challenge = await client.securityPasskeyRegisterBegin({ label, step_up: stepUp });
      const credential = await performRegistrationCeremony(challenge.ceremony);
      const passkey = await client.securityPasskeyRegisterFinish(challenge.registration_id, {
        credential,
        step_up: stepUp,
      });
      setSnapshot((snapshot) => ({
        ...snapshot,
        passkeys: newestFirst([...snapshot.passkeys, passkey]),
      }));
      setPasskeyLabel("");
      setNotice(copyEntry("security.passkeys.added").message);
    });
  };

  const revokePasskey = async (passkeyId: string) => {
    await runMutation(`passkey:${passkeyId}`, async () => {
      const stepUp = await securityStepUp(client, "revoke-passkey", passkeyId, authenticator);
      const result = await client.securityPasskeyRevoke(passkeyId, { step_up: stepUp });
      setSnapshot((snapshot) => ({ ...snapshot, passkeys: result.passkeys }));
      setNotice(copyEntry("security.passkeys.removed").message);
    });
  };

  const revokeSession = async (session: Session) => {
    await runMutation(`session:${session.session_id}`, async () => {
      const stepUp = await securityStepUp(client, "revoke-session", session.session_id, authenticator);
      const result = await client.securitySessionRevoke(
        session.session_id,
        { step_up: stepUp },
        `security-session-revoke:${stepUp.challenge_id}`,
      );
      setSnapshot((snapshot) => ({
        ...snapshot,
        sessions: snapshot.sessions.filter(
          (candidate) => !result.revoked_session_ids.includes(candidate.session_id),
        ),
      }));
      if (session.current) {
        window.location.assign("/?return_to=%2Fapp%2Fsettings%2Fsecurity");
        return;
      }
      setNotice(copyEntry("security.sessions.revoked").message);
    });
  };

  const signOutEverywhere = async () => {
    await runMutation("sessions:all", async () => {
      const stepUp = await securityStepUp(client, "revoke-all-sessions", undefined, authenticator);
      await client.securitySessionRevokeAll(
        { step_up: stepUp },
        `security-session-revoke-all:${stepUp.challenge_id}`,
      );
      window.location.assign("/?return_to=%2Fapp%2Fsettings%2Fsecurity");
    });
  };

  const beginAuthenticatorSetup = async () => {
    const label = authenticatorLabel.trim();
    await runMutation("authenticator:add", async () => {
      const stepUp = await securityStepUp(client, "add-authenticator", label, authenticator);
      const challenge = await client.authenticatorSetupBegin({ label, step_up: stepUp });
      setAuthenticatorSetup({ label, challenge, stepUp });
      setNotice(undefined);
    });
  };

  const finishAuthenticatorSetup = async () => {
    if (authenticatorSetup === undefined) {
      return;
    }
    await runMutation("authenticator:finish", async () => {
      const result = await client.authenticatorSetupFinish(authenticatorSetup.challenge.setup_id, {
        code: authenticatorCode,
        step_up: authenticatorSetup.stepUp,
      });
      setSnapshot((snapshot) => ({
        ...snapshot,
        authenticators: {
          ...snapshot.authenticators,
          methods: newestFirst([...snapshot.authenticators.methods, result.method]),
          backup_codes_remaining: result.backup_codes.codes.length,
        },
      }));
      setBackupCodes(result.backup_codes);
      setAuthenticatorSetup(undefined);
      setAuthenticatorCode("");
      setAuthenticatorLabel("");
      setNotice(copyEntry("security.authenticator.enabled").message);
    });
  };

  const disableAuthenticator = async (authenticatorId: string) => {
    await runMutation(`authenticator:disable:${authenticatorId}`, async () => {
      const stepUp = await securityStepUp(
        client,
        "disable-authenticator",
        authenticatorId,
        authenticator,
      );
      const status = await client.authenticatorDisable(authenticatorId, { step_up: stepUp });
      setSnapshot((snapshot) => ({ ...snapshot, authenticators: status }));
      setDisableCandidate(undefined);
      setNotice(copyEntry("security.authenticator.disabled").message);
    });
  };

  const rotateBackupCodes = async () => {
    await runMutation("backup:rotate", async () => {
      const stepUp = await securityStepUp(client, "rotate-backup-codes", undefined, authenticator);
      const codes = await client.authenticatorBackupRotate({ step_up: stepUp });
      setBackupCodes(codes);
      setSnapshot((snapshot) => ({
        ...snapshot,
        authenticators: {
          ...snapshot.authenticators,
          backup_codes_remaining: codes.codes.length,
        },
      }));
      setNotice(undefined);
    });
  };

  const revealRecovery = async (evidenceId: string) => {
    await runMutation("recovery:reveal", async () => {
      const stepUp = await securityStepUp(
        client,
        "reveal-recovery-evidence",
        evidenceId,
        authenticator,
      );
      const secret = await client.securityRecoveryReveal({ evidence_id: evidenceId, step_up: stepUp });
      setRecoverySecret(secret);
      setNotice(undefined);
    });
  };

  const expireAuthenticatorSetup = useCallback(() => {
    setAuthenticatorSetup(undefined);
    setNotice(copyEntry("security.secret.remasked").message);
  }, []);
  const expireBackupCodes = useCallback(() => {
    setBackupCodes(undefined);
    setNotice(copyEntry("security.secret.remasked").message);
  }, []);
  const expireRecoverySecret = useCallback(() => {
    setRecoverySecret(undefined);
    setNotice(copyEntry("security.secret.remasked").message);
  }, []);

  if (loadState.state === "loading") {
    return <LoadingSurface rows={8} />;
  }
  if (loadState.state === "offline") {
    return <OfflineSurface onRetry={() => { void load(); }} />;
  }
  if (loadState.state === "error") {
    return (
      <ErrorSurface
        error={errorPresentation(loadState.error)}
        route="/app/settings/security"
        onRetry={() => { void load(); }}
        onReload={() => { window.location.reload(); }}
      />
    );
  }

  const snapshot = loadState.snapshot;
  const recovery = recoveryPresentation(snapshot.onboarding);
  const recoveryDetails = recovery.receipt === undefined ? (
    <p className="py-2 text-sm text-muted-foreground">
      {copyEntry("security.recovery.none").message}
    </p>
  ) : (
    <div className="flex flex-col gap-2 py-2">
      <span className="text-sm font-semibold">
        {copyEntry("security.recovery.receipt").message}
      </span>
      <code aria-label={copyEntry("security.secret.hidden").message}>•••• •••• ••••</code>
      <KitButton
        type="button"
        variant="secondary"
        loading={busy === "recovery:reveal"}
        onClick={() => { void revealRecovery(recovery.receipt?.evidence_id ?? ""); }}
      >
        {copyEntry("security.recovery.reveal").message}
      </KitButton>
      {recoverySecret === undefined ? null : (
        <TimedSecretView
          label={copyEntry("security.recovery.receipt").message}
          secret={recoverySecret}
          onExpired={expireRecoverySecret}
        />
      )}
    </div>
  );

  return (
    <ScreenCard
      title={copyEntry("security.title").message}
      description={copyEntry("security.summary").message}
      dataApplication="security"
    >
      <div className="mt-4 flex flex-col gap-4">
        {notice === undefined ? null : (
          <InlineNotice tone={notice === copyEntry("security.mutation.failed").message ? "warning" : "success"}>
            {notice}
          </InlineNotice>
        )}

        <SettingsSection title={copyEntry("security.passkeys.title").message}>
          <SettingsRow
            title={copyEntry("security.passkeys.body").message}
            subtitle={copyEntry("security.passkeys.add").message}
          />
          {newestFirst(snapshot.passkeys).map((passkey) => (
            <SettingsRow
              key={passkey.passkey_id}
              title={passkey.label}
              subtitle={formatLastActive(passkey.last_used_at ?? passkey.created_at)}
              trailing={(
                <KitButton
                  type="button"
                  variant="secondary"
                  size="sm"
                  loading={busy === `passkey:${passkey.passkey_id}`}
                  {...(snapshot.passkeys.length <= 1
                    ? {
                        disabled: true as const,
                        disabledReason: copyEntry("security.passkeys.last").message,
                      }
                    : {})}
                  onClick={() => { void revokePasskey(passkey.passkey_id); }}
                >
                  {copyEntry("security.passkeys.remove").message}
                </KitButton>
              )}
            />
          ))}
          <div className="flex flex-col gap-2 py-3">
            <label className="flex flex-col gap-1 text-sm font-semibold text-foreground">
              {copyEntry("security.passkeys.label").message}
              <SettingsTextInput
                value={passkeyLabel}
                maxLength={128}
                autoComplete="off"
                onChange={(event) => { setPasskeyLabel(event.target.value); }}
              />
            </label>
            <KitButton
              type="button"
              loading={busy === "add-passkey"}
              {...(passkeyLabel.trim().length === 0
                ? {
                    disabled: true as const,
                    disabledReason: copyEntry("security.passkeys.label_required").message,
                  }
                : {})}
              onClick={() => { void addPasskey(); }}
            >
              {copyEntry("security.passkeys.add").message}
            </KitButton>
          </div>
        </SettingsSection>

        <SettingsSection title={copyEntry("security.sessions.title").message}>
          <DeviceSessionList
            items={snapshot.sessions.map((session) => ({
              id: session.session_id,
              title: session.device.label,
              subtitle: formatCopy("security.sessions.last_active", {
                date: formatLastActive(session.last_active_at),
              }),
              current: session.current,
              trailingCaption: session.current ? copyEntry("security.sessions.current").message : undefined,
              trailing: (
                <KitButton
                  type="button"
                  variant="secondary"
                  size="sm"
                  loading={busy === `session:${session.session_id}`}
                  onClick={() => { void revokeSession(session); }}
                >
                  {copyEntry("security.sessions.revoke").message}
                </KitButton>
              ),
            }))}
          />
          {confirmSignOutEverywhere ? (
            <div className="flex flex-col gap-2 py-3">
              <InlineNotice tone="warning">
                {copyEntry("security.sessions.revoke_all_warning").message}
              </InlineNotice>
              <div className="flex flex-wrap gap-2">
                <KitButton
                  type="button"
                  variant="destructive"
                  loading={busy === "sessions:all"}
                  onClick={() => { void signOutEverywhere(); }}
                >
                  {copyEntry("security.sessions.confirm_revoke_all").message}
                </KitButton>
                <KitButton type="button" variant="secondary" onClick={() => { setConfirmSignOutEverywhere(false); }}>
                  {copyEntry("action.cancel").message}
                </KitButton>
              </div>
            </div>
          ) : (
            <div className="py-3">
              <KitButton type="button" variant="destructive" onClick={() => { setConfirmSignOutEverywhere(true); }}>
                {copyEntry("security.sessions.revoke_all").message}
              </KitButton>
            </div>
          )}
        </SettingsSection>

        <SettingsSection title={copyEntry("security.authenticator.title").message}>
          <SettingsRow
            title={copyEntry("security.authenticator.body").message}
            subtitle={formatCopy("security.backup.remaining", {
              count: snapshot.authenticators.backup_codes_remaining,
            })}
          />
          {newestFirst(snapshot.authenticators.methods).map((method) => (
            <div key={method.authenticator_id}>
              <SettingsRow
                title={method.label}
                subtitle={formatLastActive(method.last_used_at ?? method.enabled_at)}
                trailing={(
                  <KitButton
                    type="button"
                    variant="secondary"
                    size="sm"
                    onClick={() => { setDisableCandidate(method.authenticator_id); }}
                  >
                    {copyEntry("security.authenticator.disable").message}
                  </KitButton>
                )}
              />
              {disableCandidate === method.authenticator_id ? (
                <div className="flex flex-col gap-2 py-3">
                  <InlineNotice tone="warning">
                    {copyEntry("security.authenticator.disable_warning").message}
                  </InlineNotice>
                  <div className="flex flex-wrap gap-2">
                    <KitButton
                      type="button"
                      variant="destructive"
                      loading={busy === `authenticator:disable:${method.authenticator_id}`}
                      onClick={() => { void disableAuthenticator(method.authenticator_id); }}
                    >
                      {copyEntry("security.authenticator.confirm_disable").message}
                    </KitButton>
                    <KitButton type="button" variant="secondary" onClick={() => { setDisableCandidate(undefined); }}>
                      {copyEntry("action.cancel").message}
                    </KitButton>
                  </div>
                </div>
              ) : null}
            </div>
          ))}

          {authenticatorSetup === undefined ? (
            <div className="flex flex-col gap-2 py-3">
              <label className="flex flex-col gap-1 text-sm font-semibold text-foreground">
                {copyEntry("security.authenticator.label").message}
                <SettingsTextInput
                  value={authenticatorLabel}
                  maxLength={128}
                  autoComplete="off"
                  onChange={(event) => { setAuthenticatorLabel(event.target.value); }}
                />
              </label>
              <KitButton
                type="button"
                loading={busy === "authenticator:add"}
                {...(authenticatorLabel.trim().length === 0
                  ? {
                      disabled: true as const,
                      disabledReason: copyEntry("security.passkeys.label_required").message,
                    }
                  : {})}
                onClick={() => { void beginAuthenticatorSetup(); }}
              >
                {copyEntry("security.authenticator.add").message}
              </KitButton>
            </div>
          ) : (
            <div className="flex flex-col gap-3 py-3">
              <InlineNotice tone="neutral">
                {copyEntry("security.authenticator.setup_body").message}
              </InlineNotice>
              <TimedSecretView
                label={copyEntry("security.authenticator.setup").message}
                secret={authenticatorSetup.challenge.otpauth_uri}
                onExpired={expireAuthenticatorSetup}
              />
              <TimedSecretView
                label={copyEntry("security.secret.hidden").message}
                secret={authenticatorSetup.challenge.secret}
                onExpired={expireAuthenticatorSetup}
              />
              <label className="flex flex-col gap-1 text-sm font-semibold text-foreground">
                {copyEntry("security.authenticator.code").message}
                <SettingsTextInput
                  value={authenticatorCode}
                  inputMode="numeric"
                  autoComplete="one-time-code"
                  maxLength={8}
                  onChange={(event) => { setAuthenticatorCode(event.target.value.replaceAll(/\D/g, "")); }}
                />
              </label>
              <KitButton
                type="button"
                loading={busy === "authenticator:finish"}
                {...(authenticatorCode.length < 6
                  ? {
                      disabled: true as const,
                      disabledReason: copyEntry("security.authenticator.code_required").message,
                    }
                  : {})}
                onClick={() => { void finishAuthenticatorSetup(); }}
              >
                {copyEntry("security.authenticator.confirm").message}
              </KitButton>
            </div>
          )}

          {snapshot.authenticators.methods.length > 0 ? (
            <div className="py-3">
              <KitButton
                type="button"
                variant="secondary"
                loading={busy === "backup:rotate"}
                onClick={() => { void rotateBackupCodes(); }}
              >
                {copyEntry("security.backup.rotate").message}
              </KitButton>
            </div>
          ) : null}
          {backupCodes === undefined ? null : (
            <BackupCodesView
              codes={backupCodes.codes}
              remaskAt={backupCodes.remask_at}
              copyable={backupCodes.copyable}
              onExpired={expireBackupCodes}
            />
          )}
        </SettingsSection>

        <SettingsSection title={copyEntry("security.recovery.title").message}>
          <SettingsRow
            title={recovery.ready
              ? copyEntry("security.recovery.ready").message
              : copyEntry("security.recovery.pending").message}
            subtitle={recovery.receipt?.verification}
          />
          {shell === "mobile" ? (
            <div className="py-3">
              <KitButton
                type="button"
                variant="secondary"
                onClick={() => { setRecoveryDetailsOpen(true); }}
              >
                {copyEntry("security.recovery.details").message}
              </KitButton>
              <MobileDetail
                open={recoveryDetailsOpen}
                onOpenChange={setRecoveryDetailsOpen}
                title={copyEntry("security.recovery.details").message}
              >
                {recoveryDetails}
              </MobileDetail>
            </div>
          ) : (
            <DesktopDetail
              open={recoveryDetailsOpen}
              onOpenChange={setRecoveryDetailsOpen}
              title={copyEntry("security.recovery.details").message}
              summary={copyEntry("security.recovery.details").message}
              desktopVariant="inline"
            >
              {recoveryDetails}
            </DesktopDetail>
          )}
        </SettingsSection>
      </div>
    </ScreenCard>
  );
}
