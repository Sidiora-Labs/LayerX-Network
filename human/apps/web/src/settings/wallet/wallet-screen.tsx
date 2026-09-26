"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useState } from "react";

import { copyEntry, human_copy_catalog } from "../../../copy/runtime.ts";
import { humanApi, type HumanApiClient } from "../../api/index.ts";
import { AddressQrCode } from "../../kit/qr-code";
import { CopyableIdentifier, LabelValue, StatusPill } from "../../kit/money";
import { InlineNotice, ScreenCard } from "../../kit/surface";
import { KitButton } from "../../kit/control";
import { SettingsSection } from "../../kit/settings";
import {
  JourneyTechnicalDetails,
  JourneyTimelineView,
} from "../../journeys/custody/timeline.tsx";
import { journeyTimeline } from "../../journeys/custody/model.ts";
import { windowWalletProvider } from "../../journeys/custody/handoff.ts";
import { useNotificationCenter } from "../../journeys/notifications/store.tsx";
import { errorPresentation, ErrorSurface } from "../../states/error.tsx";
import { LoadingSurface, OfflineSurface } from "../../states/surfaces.tsx";
import { formatLastActive } from "../security/model.ts";
import { browserBindingWalletBridge } from "./bridge.ts";
import {
  WalletBindingController,
  WalletFeeController,
  type WalletFeeSnapshot,
  newestWalletSecurityNotification,
  type WalletBindingSnapshot,
} from "./model.ts";

const REFRESH_INTERVAL_MS = 4_000;

function phaseCopy(phase: WalletBindingSnapshot["phase"]): Readonly<{
  titleKey: string;
  bodyKey: string;
  tone: "neutral" | "warning" | "danger" | "success";
}> {
  switch (phase) {
    case "ready":
    case "active":
      return { titleKey: "settings.wallet.bound", bodyKey: "settings.wallet.active.body", tone: "success" };
    case "waiting":
      return { titleKey: "settings.wallet.handoff.in_progress", bodyKey: "settings.wallet.handoff.in_progress.body", tone: "neutral" };
    case "cancelled":
      return { titleKey: "wallet.handoff.cancelled", bodyKey: "settings.wallet.handoff.cancelled.body", tone: "warning" };
    case "rejected":
      return { titleKey: "wallet.handoff.rejected", bodyKey: "settings.wallet.handoff.rejected.body", tone: "warning" };
    case "unavailable":
      return { titleKey: "wallet.handoff.unavailable", bodyKey: "settings.wallet.handoff.unavailable.body", tone: "warning" };
    case "failed":
      return { titleKey: "wallet.handoff.failed", bodyKey: "settings.wallet.handoff.failed.body", tone: "danger" };
    case "submitted":
      return { titleKey: "settings.wallet.rebinding", bodyKey: "settings.wallet.rebinding.body", tone: "neutral" };
  }
}

export function WalletBindingScreen({
  client: suppliedClient,
}: Readonly<{ client?: HumanApiClient }>) {
  const router = useRouter();
  const notifications = useNotificationCenter();
  const client = useMemo(() => suppliedClient ?? humanApi(), [suppliedClient]);
  const controller = useMemo(() => new WalletBindingController({
    client,
    bridge: browserBindingWalletBridge(windowWalletProvider),
  }), [client]);
  const [snapshot, setSnapshot] = useState<WalletBindingSnapshot | undefined>(undefined);
  const [failure, setFailure] = useState<unknown>(undefined);
  const refresh = useCallback(async () => {
    try {
      setSnapshot(await controller.refresh());
    } catch (error) {
      setFailure(error);
    }
  }, [controller]);

  useEffect(() => {
    let cancelled = false;
    void controller.load().then((loaded) => {
      if (!cancelled) {
        setSnapshot(loaded);
      }
    }).catch((error: unknown) => {
      if (!cancelled) {
        setFailure(error);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [controller]);

  useEffect(() => {
    if (snapshot?.phase !== "submitted") {
      return;
    }
    const timer = window.setInterval(() => { void refresh(); }, REFRESH_INTERVAL_MS);
    return () => { window.clearInterval(timer); };
  }, [refresh, snapshot?.phase]);

  useEffect(() => {
    if (snapshot?.phase === "active" && snapshot.candidate !== undefined) {
      void notifications.refresh();
    }
  }, [notifications, snapshot?.candidate, snapshot?.phase]);

  if (failure !== undefined) {
    if (!navigator.onLine) {
      return <OfflineSurface onRetry={() => { window.location.reload(); }} />;
    }
    return (
      <ErrorSurface
        error={errorPresentation(failure)}
        route="/app/settings/wallet"
        onRetry={() => {
          setFailure(undefined);
          void controller.load().then(setSnapshot).catch(setFailure);
        }}
        onReload={() => { window.location.reload(); }}
      />
    );
  }

  if (snapshot === undefined) {
    return <LoadingSurface rows={4} />;
  }

  const active = snapshot.active;
  if (active === undefined) {
    return (
      <ScreenCard
        title={copyEntry("settings.wallet.title").message}
        description={copyEntry("settings.wallet.unbound.body").message}
        dataApplication="wallet-binding"
      >
        <div className="flex flex-col gap-4 pt-4">
          <WalletFeeSettings />
          <InlineNotice tone="neutral">{copyEntry("settings.wallet.no_authority").message}</InlineNotice>
          {snapshot.status?.state === "binding" ? (
            <InlineNotice tone="neutral">{copyEntry("settings.wallet.binding.body").message}</InlineNotice>
          ) : null}
          <KitButton variant="primary" onClick={() => { router.push("/app/deposit"); }}>
            {copyEntry("settings.wallet.add_money").message}
          </KitButton>
        </div>
      </ScreenCard>
    );
  }

  const presentation = phaseCopy(snapshot.phase);
  const notificationSources = notifications.state.status === "ready"
    ? notifications.state.notifications.map((notification) => notification.source)
    : [];
  const securityNotification = newestWalletSecurityNotification(notificationSources);
  const actionKey = securityNotification?.action_copy_key;
  const canRetry = snapshot.phase === "cancelled"
    || snapshot.phase === "rejected"
    || snapshot.phase === "unavailable"
    || snapshot.phase === "failed";

  const runRebind = async () => {
    setFailure(undefined);
    const pending = controller.rebind();
    setSnapshot(controller.snapshot);
    try {
      setSnapshot(await pending);
    } catch (error) {
      setFailure(error);
    }
  };

  return (
    <ScreenCard
      title={copyEntry("settings.wallet.title").message}
      description={copyEntry("settings.wallet.summary").message}
      dataApplication="wallet-binding"
    >
      <div className="flex flex-col gap-4 pt-4">
        <InlineNotice tone="neutral">{copyEntry("settings.wallet.no_authority").message}</InlineNotice>
        <WalletFeeSettings />
        <SettingsSection title={copyEntry("settings.wallet.current").message}>
          <LabelValue
            label={copyEntry("settings.wallet.linked_at").message}
            value={formatLastActive(active.boundAt)}
          />
          <CopyableIdentifier
            label={copyEntry("settings.wallet.address").message}
            value={active.address}
          />
          <div className="flex justify-center py-3">
            <AddressQrCode value={active.address} label={copyEntry("settings.wallet.qr").message} />
          </div>
          <LabelValue
            label={copyEntry("settings.wallet.verification").message}
            value={copyEntry("settings.wallet.receipt_verified").message}
          />
        </SettingsSection>

        {snapshot.phase === "active" && snapshot.candidate === undefined ? null : (
          <InlineNotice tone={presentation.tone} role={presentation.tone === "danger" ? "alert" : "status"}>
            <span className="font-semibold">{copyEntry(presentation.titleKey).message}</span>{" "}
            {copyEntry(presentation.bodyKey).message}
          </InlineNotice>
        )}

        {snapshot.candidate === undefined ? null : (
          <CopyableIdentifier
            label={copyEntry("settings.wallet.pending_address").message}
            value={snapshot.candidate}
          />
        )}

        {snapshot.journey === undefined ? null : (
          <>
            <StatusPill status={snapshot.phase === "active" ? "done" : "processing"} />
            <JourneyTimelineView rows={journeyTimeline(snapshot.journey)} />
            <JourneyTechnicalDetails journey={snapshot.journey} />
          </>
        )}

        {snapshot.phase === "waiting" ? (
          <KitButton
            variant="secondary"
            onClick={() => {
              controller.cancel();
              setSnapshot(controller.snapshot);
            }}
          >
            {copyEntry("settings.wallet.cancel").message}
          </KitButton>
        ) : snapshot.phase === "submitted" ? null : (
          <KitButton variant="primary" onClick={() => { void runRebind(); }}>
            {copyEntry(canRetry ? "settings.wallet.retry" : "settings.wallet.change").message}
          </KitButton>
        )}

        {securityNotification === undefined ? null : (
          <InlineNotice tone="warning">
            <span className="font-semibold">
              {copyEntry("notification.security-wallet-rebinding.title").message}
            </span>{" "}
            {copyEntry("settings.wallet.security_notification.body").message}{" "}
            {actionKey === undefined || !human_copy_catalog().has(actionKey) ? null : (
              <KitButton
                type="button"
                variant="link"
                size="sm"
                onClick={() => { router.push(securityNotification.deep_link); }}
              >
                {copyEntry(actionKey).message}
              </KitButton>
            )}
          </InlineNotice>
        )}
      </div>
    </ScreenCard>
  );
}


export function WalletFeeChoice({ snapshot, onChange }: Readonly<{
  snapshot: WalletFeeSnapshot;
  onChange: (currency: WalletFeeSnapshot["currency"], maximum: string) => void;
}>) {
  return (
    <fieldset className="flex flex-col gap-3 border border-border p-4">
      <legend>{copyEntry("gas.sidiora.title").message}</legend>
      <p>{copyEntry("gas.sidiora.scope").message}</p>
      <label className="flex items-center gap-2">
        <input type="radio" name="fee-currency" checked={snapshot.currency === "paxeer"}
          onChange={() => { onChange("paxeer", snapshot.maximum); }} />
        {copyEntry("gas.sidiora.paxeer").message}
      </label>
      <label className="flex items-center gap-2">
        <input type="radio" name="fee-currency" checked={snapshot.currency === "sidiora"}
          onChange={() => { onChange("sidiora", snapshot.maximum); }} />
        {copyEntry("gas.sidiora.choose").message}
      </label>
      {snapshot.currency === "sidiora" ? (
        <>
          <label className="flex flex-col gap-2">
            {copyEntry("gas.sidiora.maximum").message}
            <input inputMode="decimal" value={snapshot.maximum} aria-invalid={!snapshot.valid}
              className="border border-border bg-transparent p-2"
              onChange={(event) => { onChange("sidiora", event.target.value); }} />
          </label>
          <p>{copyEntry("gas.sidiora.review").message}</p>
          {snapshot.valid ? null : <p role="alert">{copyEntry("gas.sidiora.invalid").message}</p>}
        </>
      ) : null}
    </fieldset>
  );
}

export function WalletFeeSettings() {
  const [controller] = useState(() => new WalletFeeController());
  const [snapshot, setSnapshot] = useState(controller.snapshot);
  return <WalletFeeChoice snapshot={snapshot} onChange={(currency, maximum) => {
    setSnapshot(controller.choose(currency, maximum));
  }} />;
}
