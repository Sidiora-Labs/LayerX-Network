"use client";

import { useRouter } from "next/navigation";
import { useEffect, useMemo, useRef, useState } from "react";

import { copyEntry, human_copy_catalog } from "../../../copy/catalog";
import {
  HumanApiError,
  humanApi,
  type Journey,
  type NotificationSummary,
  type Session,
} from "../../api";
import { ACTIVE_ACCOUNT_STORAGE_KEY } from "../../auth/session";
import {
  DesktopPrimaryAction,
  InlineNotice,
  KitButton,
  MobilePrimaryAction,
  ScreenCard,
  StateSkeleton,
  StatusPill,
  TextField,
} from "../../kit";
import { ShellSelector, type PointerCapability, type ShellSelection } from "../../shell/selector";
import { CeremonyCancelled, performAssertionCeremony, performRegistrationCeremony } from "./ceremony";
import {
  ONBOARDING_DECISION_LIMIT,
  accountActive,
  stagePresentation,
} from "./model";

interface Failure {
  readonly messageKey: string;
  readonly retriable: boolean;
  readonly field?: string;
}

type Phase =
  | Readonly<{ name: "details" }>
  | Readonly<{ name: "passkey"; accountId: string; passkeyAdded: boolean }>
  | Readonly<{ name: "progress" }>
  | Readonly<{ name: "signin" }>
  | Readonly<{ name: "signedIn"; notice?: NotificationSummary }>;

interface PendingOnboarding {
  readonly version: 1;
  readonly idempotencyKey: string;
  readonly email: string;
  readonly displayName: string;
  readonly accountId?: string;
  readonly passkeyAdded: boolean;
}

const PENDING_ONBOARDING_KEY = "layerx.onboarding.pending.v1";

function pendingOnboarding(): PendingOnboarding | undefined {
  let decoded: unknown;
  try {
    const stored = localStorage.getItem(PENDING_ONBOARDING_KEY);
    if (stored === null) {
      return undefined;
    }
    decoded = JSON.parse(stored);
  } catch {
    return undefined;
  }
  if (typeof decoded !== "object" || decoded === null || Array.isArray(decoded)) {
    return undefined;
  }
  const candidate = decoded as Partial<PendingOnboarding>;
  if (
    candidate.version !== 1
    || typeof candidate.idempotencyKey !== "string"
    || typeof candidate.email !== "string"
    || typeof candidate.displayName !== "string"
    || typeof candidate.passkeyAdded !== "boolean"
    || (candidate.accountId !== undefined && typeof candidate.accountId !== "string")
  ) {
    return undefined;
  }
  return candidate as PendingOnboarding;
}

function rememberPending(pending: PendingOnboarding): void {
  try {
    localStorage.setItem(PENDING_ONBOARDING_KEY, JSON.stringify(pending));
  } catch {
    return;
  }
}

function forgetPending(): void {
  try {
    localStorage.removeItem(PENDING_ONBOARDING_KEY);
  } catch {
    return;
  }
}

function rememberActivatedAccount(journey: Journey): void {
  if (!accountActive(journey)) {
    return;
  }
  const pending = pendingOnboarding();
  if (pending?.accountId === undefined) {
    return;
  }
  try {
    localStorage.setItem(ACTIVE_ACCOUNT_STORAGE_KEY, pending.accountId);
  } catch {
    return;
  }
  forgetPending();
}

function failureFromError(error: unknown): Failure {
  if (error instanceof HumanApiError) {
    return {
      messageKey: error.detail.copy_key,
      retriable: error.detail.retry === "retriable" || error.detail.retry === "retriable-after",
      ...(error.detail.field === undefined ? {} : { field: error.detail.field }),
    };
  }
  if (error instanceof CeremonyCancelled) {
    return { messageKey: "onboarding.ceremony.cancelled", retriable: true };
  }
  if (typeof navigator !== "undefined" && !navigator.onLine) {
    return { messageKey: "state.offline.body", retriable: true };
  }
  return { messageKey: "state.error.body", retriable: true };
}

function sessionEnded(error: unknown): boolean {
  return error instanceof HumanApiError
    && (error.detail.code === "unauthenticated" || error.detail.code === "session-expired");
}

function failureMessage(messageKey: string): string {
  return human_copy_catalog().get(messageKey)?.message ?? copyEntry("state.error.body").message;
}

function StageList({ journey }: Readonly<{ journey: Journey }>) {
  return (
    <ul data-onboarding-stages="" className="flex list-none flex-col divide-y divide-border">
      {journey.stages.map((current) => {
        const presented = stagePresentation(current);
        return (
          <li
            key={presented.stageId}
            data-stage-key={presented.copyKey}
            data-stage-state={presented.state}
            className="flex items-center justify-between gap-4 py-3"
          >
            <span className="text-[15px] text-foreground">{presented.title}</span>
            <StatusPill status={presented.status} />
          </li>
        );
      })}
    </ul>
  );
}

function FailureNotice({
  failure,
  action,
}: Readonly<{ failure: Failure; action?: React.ReactNode }>) {
  return (
    <InlineNotice tone="danger" role="alert">
      <div data-onboarding-failure={failure.messageKey} className="flex flex-col gap-2">
        <p className="font-semibold">{copyEntry("onboarding.failure.title").message}</p>
        <p>{failureMessage(failure.messageKey)}</p>
        <p>{copyEntry("onboarding.failure.preserved").message}</p>
        {action}
      </div>
    </InlineNotice>
  );
}

export function Onboarding({
  initialSelection,
  initiallyAuthenticated = false,
  returnTo,
}: Readonly<{
  initialSelection: ShellSelection;
  initiallyAuthenticated?: boolean;
  returnTo?: string | undefined;
}>) {
  const router = useRouter();
  const api = useMemo(() => humanApi(), []);
  const [selection, setSelection] = useState(initialSelection);
  const [phase, setPhase] = useState<Phase>(initiallyAuthenticated ? { name: "progress" } : { name: "details" });
  const [journey, setJourney] = useState<Journey | undefined>(undefined);
  const [failure, setFailure] = useState<Failure | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const creationKey = useRef<string | undefined>(undefined);

  useEffect(() => {
    const pending = pendingOnboarding();
    if (pending === undefined) {
      return;
    }
    creationKey.current = pending.idempotencyKey;
    setEmail(pending.email);
    setDisplayName(pending.displayName);
    if (!initiallyAuthenticated && pending.accountId !== undefined) {
      setPhase({
        name: "passkey",
        accountId: pending.accountId,
        passkeyAdded: pending.passkeyAdded,
      });
    }
  }, [initiallyAuthenticated]);

  useEffect(() => {
    const pointer: PointerCapability = window.matchMedia("(pointer: coarse)").matches
      ? "coarse"
      : window.matchMedia("(pointer: fine)").matches
        ? "fine"
        : "none";
    const confirmation = ShellSelector.confirm(initialSelection, {
      viewportWidth: window.innerWidth,
      pointer,
    });
    setSelection(confirmation.selection);
  }, [initialSelection]);

  useEffect(() => {
    if (phase.name !== "progress") {
      return;
    }
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      try {
        const next = await api.onboardingStatus();
        if (!cancelled) {
          rememberActivatedAccount(next);
          setJourney(next);
          setFailure(undefined);
        }
      } catch (error) {
        if (!cancelled) {
          if (sessionEnded(error)) {
            setPhase({ name: "signin" });
          } else {
            setFailure(failureFromError(error));
          }
        }
      }
      if (!cancelled) {
        timer = setTimeout(() => {
          void poll();
        }, 2_500);
      }
    };
    void poll();
    return () => {
      cancelled = true;
      if (timer !== undefined) {
        clearTimeout(timer);
      }
    };
  }, [api, phase.name]);

  const PrimaryAction = selection.shell === "mobile" ? MobilePrimaryAction : DesktopPrimaryAction;
  const busyReason = copyEntry("state.loading").message;

  const appDestination = (candidate: string | undefined): string => {
    if (candidate === undefined) {
      return "/app";
    }
    try {
      return ShellSelector.resolveDeepLink(candidate, selection.shell).href;
    } catch {
      return "/app";
    }
  };

  const run = (work: () => Promise<void>): void => {
    setBusy(true);
    setFailure(undefined);
    void work()
      .catch((error: unknown) => {
        setFailure(failureFromError(error));
      })
      .finally(() => {
        setBusy(false);
      });
  };

  const submitDetails = (): void => {
    run(async () => {
      creationKey.current ??= crypto.randomUUID();
      const normalizedEmail = email.trim().toLowerCase();
      const normalizedName = displayName.trim();
      rememberPending({
        version: 1,
        idempotencyKey: creationKey.current,
        email: normalizedEmail,
        displayName: normalizedName,
        passkeyAdded: false,
      });
      const creation = await api.accountCreate(
        { email: normalizedEmail, display_name: normalizedName },
        creationKey.current,
      );
      rememberPending({
        version: 1,
        idempotencyKey: creationKey.current,
        email: normalizedEmail,
        displayName: normalizedName,
        accountId: creation.account_id,
        passkeyAdded: false,
      });
      setJourney(creation.onboarding);
      setPhase({ name: "passkey", accountId: creation.account_id, passkeyAdded: false });
    });
  };

  const openSession = async (assertionEmail: string | undefined): Promise<Session> => {
    const request = assertionEmail === undefined ? {} : { email: assertionEmail };
    const challenge = await api.passkeyAssertBegin(request);
    const credential = await performAssertionCeremony(challenge.ceremony);
    const completed = await api.passkeyAssertFinish(challenge.assertion_id, { credential });
    return api.sessionOpen(
      {
        assertion_id: completed.assertion_id,
        device: { label: "LayerX web app", platform: "web" },
      },
      `session-open:${completed.assertion_id}`,
    );
  };

  const continueSetup = (phaseNow: Extract<Phase, { name: "passkey" }>): void => {
    run(async () => {
      if (!phaseNow.passkeyAdded) {
        const challenge = await api.passkeyRegisterBegin({ account_id: phaseNow.accountId });
        const credential = await performRegistrationCeremony(challenge.ceremony);
        await api.passkeyRegisterFinish(challenge.registration_id, { credential });
        const idempotencyKey = creationKey.current ?? crypto.randomUUID();
        creationKey.current = idempotencyKey;
        rememberPending({
          version: 1,
          idempotencyKey,
          email: email.trim().toLowerCase(),
          displayName: displayName.trim(),
          accountId: phaseNow.accountId,
          passkeyAdded: true,
        });
        setPhase({ ...phaseNow, passkeyAdded: true });
      }
      const assertionEmail = email.trim().toLowerCase();
      await openSession(assertionEmail.length === 0 ? undefined : assertionEmail);
      setPhase({ name: "progress" });
      const next = await api.onboardingStatus();
      rememberActivatedAccount(next);
      setJourney(next);
    });
  };

  const signIn = (): void => {
    run(async () => {
      const trimmed = email.trim().toLowerCase();
      const opened = await openSession(trimmed.length === 0 ? undefined : trimmed);
      setPhase({ name: "progress" });
      const [next, page] = await Promise.all([api.onboardingStatus(), api.notificationList()]);
      rememberActivatedAccount(next);
      setJourney(next);
      const openedAt = Date.parse(opened.opened_at);
      const notice = page.groups
        .flatMap((group) => group.notifications)
        .find((candidate) => candidate.class === "security-new-device"
          && !candidate.read
          && Date.parse(candidate.created_at) >= openedAt - 60_000);
      if (accountActive(next)) {
        setPhase({ name: "signedIn", ...(notice === undefined ? {} : { notice }) });
      }
    });
  };

  const resume = (): void => {
    run(async () => {
      const next = await api.onboardingResume();
      rememberActivatedAccount(next);
      setJourney(next);
    });
  };

  const returnToCreation = (): void => {
    setFailure(undefined);
    const pending = pendingOnboarding();
    if (pending?.accountId === undefined) {
      setPhase({ name: "details" });
      return;
    }
    creationKey.current = pending.idempotencyKey;
    setEmail(pending.email);
    setDisplayName(pending.displayName);
    setPhase({
      name: "passkey",
      accountId: pending.accountId,
      passkeyAdded: pending.passkeyAdded,
    });
  };

  const emailError = failure?.field === "email" ? failureMessage(failure.messageKey) : undefined;
  const nameError = failure?.field === "display_name" ? failureMessage(failure.messageKey) : undefined;
  const generalFailure = failure !== undefined && failure.field === undefined ? failure : undefined;

  const detailsScreen = (
    <ScreenCard
      title={copyEntry("onboarding.create.title").message}
      description={copyEntry("onboarding.create.body").message}
      dataApplication={copyEntry("application.name").message}
    >
      <form
        data-decision-screen="1"
        className="flex flex-col gap-4 pt-2"
        onSubmit={(event) => {
          event.preventDefault();
          submitDetails();
        }}
      >
        <TextField
          label={copyEntry("onboarding.email.label").message}
          type="email"
          name="email"
          autoComplete="email"
          inputMode="email"
          required
          value={email}
          onChange={(event) => {
            setEmail(event.target.value);
          }}
          errorMessage={emailError}
        />
        <TextField
          label={copyEntry("onboarding.name.label").message}
          type="text"
          name="display_name"
          autoComplete="name"
          required
          value={displayName}
          onChange={(event) => {
            setDisplayName(event.target.value);
          }}
          errorMessage={nameError}
        />
        {generalFailure === undefined ? null : <FailureNotice failure={generalFailure} />}
        <PrimaryAction
          type="submit"
          {...(busy ? { disabled: true, disabledReason: busyReason } : {})}
        >
          {copyEntry("onboarding.create.action").message}
        </PrimaryAction>
      </form>
      <KitButton
        variant="link"
        onClick={() => {
          setFailure(undefined);
          setPhase({ name: "signin" });
        }}
      >
        {copyEntry("onboarding.signin.switch").message}
      </KitButton>
    </ScreenCard>
  );

  const passkeyScreen = (phaseNow: Extract<Phase, { name: "passkey" }>) => (
    <ScreenCard
      title={copyEntry("onboarding.passkey.title").message}
      description={copyEntry(phaseNow.passkeyAdded
        ? "onboarding.passkey.resume.body"
        : "onboarding.passkey.body").message}
    >
      <div data-decision-screen="2" className="flex flex-col gap-4 pt-2">
        {journey === undefined ? null : <StageList journey={journey} />}
        {generalFailure === undefined ? null : <FailureNotice failure={generalFailure} />}
        <PrimaryAction
          onClick={() => {
            continueSetup(phaseNow);
          }}
          {...(busy ? { disabled: true, disabledReason: busyReason } : {})}
        >
          {copyEntry(phaseNow.passkeyAdded
            ? "onboarding.passkey.resume.action"
            : "onboarding.passkey.action").message}
        </PrimaryAction>
      </div>
    </ScreenCard>
  );

  const progressScreen = () => {
    const active = journey !== undefined && accountActive(journey);
    const queued = journey !== undefined && journey.stages.some((current) => current.state === "getting-ready");
    const refused = journey?.refusal !== undefined
      ? { messageKey: journey.refusal.copy_key, retriable: true } satisfies Failure
      : journey?.stages.some((current) => current.state === "refused") === true
        ? { messageKey: "onboarding.failure.stage", retriable: true } satisfies Failure
        : undefined;
    const progressFailure = generalFailure ?? refused;
    return (
      <ScreenCard
        title={copyEntry("onboarding.progress.title").message}
        description={copyEntry("onboarding.progress.body").message}
      >
        <div data-journey-phase="progress" className="flex flex-col gap-4 pt-2">
          <InlineNotice tone={active ? "success" : "warning"} role="status">
            <span data-account-active={active ? "true" : "false"}>
              {copyEntry(active ? "onboarding.active" : "onboarding.not_active").message}
            </span>
          </InlineNotice>
          {journey === undefined ? <StateSkeleton /> : <StageList journey={journey} />}
          {queued ? (
            <p data-onboarding-queued="" className="text-sm text-muted-foreground">
              {copyEntry("onboarding.pending.body").message}
            </p>
          ) : null}
          <p className="text-sm text-muted-foreground">
            {copyEntry("onboarding.progress.safe_to_close").message}
          </p>
          {progressFailure === undefined ? null : <FailureNotice failure={progressFailure} />}
          <div className="flex flex-wrap items-center gap-3">
            <KitButton
              variant="secondary"
              data-onboarding-resume=""
              onClick={resume}
              {...(busy ? { disabled: true, disabledReason: busyReason } : {})}
            >
              {copyEntry("onboarding.resume.action").message}
            </KitButton>
            {active ? (
              <KitButton
                variant="primary"
                onClick={() => {
                  router.push(appDestination(returnTo));
                }}
              >
                {copyEntry("onboarding.signin.continue").message}
              </KitButton>
            ) : null}
          </div>
        </div>
      </ScreenCard>
    );
  };

  const signinScreen = (
    <ScreenCard
      title={copyEntry("onboarding.signin.title").message}
      description={copyEntry("onboarding.signin.body").message}
    >
      <form
        data-decision-screen="1"
        className="flex flex-col gap-4 pt-2"
        onSubmit={(event) => {
          event.preventDefault();
          signIn();
        }}
      >
        <TextField
          label={copyEntry("onboarding.email.label").message}
          type="email"
          name="email"
          autoComplete="email"
          inputMode="email"
          required
          value={email}
          onChange={(event) => {
            setEmail(event.target.value);
          }}
          errorMessage={emailError}
        />
        {generalFailure === undefined ? null : <FailureNotice failure={generalFailure} />}
        <PrimaryAction
          type="submit"
          {...(busy ? { disabled: true, disabledReason: busyReason } : {})}
        >
          {copyEntry("onboarding.signin.action").message}
        </PrimaryAction>
      </form>
      <KitButton
        variant="link"
        onClick={returnToCreation}
      >
        {copyEntry("onboarding.create.switch").message}
      </KitButton>
    </ScreenCard>
  );

  const signedInScreen = (phaseNow: Extract<Phase, { name: "signedIn" }>) => (
    <ScreenCard title={copyEntry("onboarding.signin.done").message}>
      <div className="flex flex-col gap-4 pt-2">
        {phaseNow.notice === undefined ? null : (
          <InlineNotice tone="warning" role="status">
            <span data-security-notice={phaseNow.notice.class} className="flex flex-wrap items-center gap-3">
              <span className="font-semibold">{failureMessage(phaseNow.notice.title_copy_key)}</span>
              <span>{failureMessage(phaseNow.notice.body_copy_key)}</span>
              <KitButton
                variant="secondary"
                data-security-notice-action=""
                onClick={() => {
                  router.push(appDestination(phaseNow.notice?.deep_link));
                }}
              >
                {failureMessage(phaseNow.notice.action_copy_key ?? "notification.action.review-devices")}
              </KitButton>
            </span>
          </InlineNotice>
        )}
        {journey === undefined ? null : <StageList journey={journey} />}
        {generalFailure === undefined ? null : <FailureNotice failure={generalFailure} />}
        <PrimaryAction
          onClick={() => {
            router.push(appDestination(returnTo));
          }}
        >
          {copyEntry("onboarding.signin.continue").message}
        </PrimaryAction>
      </div>
    </ScreenCard>
  );

  return (
    <div
      data-journey="onboarding"
      data-shell={selection.shell}
      data-decision-limit={String(ONBOARDING_DECISION_LIMIT)}
    >
      {phase.name === "details" ? detailsScreen : null}
      {phase.name === "passkey" ? passkeyScreen(phase) : null}
      {phase.name === "progress" ? progressScreen() : null}
      {phase.name === "signin" ? signinScreen : null}
      {phase.name === "signedIn" ? signedInScreen(phase) : null}
    </div>
  );
}
