"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useState, type SyntheticEvent } from "react";

import { copyEntry } from "../../copy/runtime";
import { formatCopy } from "../../copy/format";
import {
  humanApi,
  type HumanApiClient,
  type NotificationClass,
  type NotificationDetailLevel,
  type NotificationPreferences,
  type Profile,
  type WalletBinding,
} from "../api";
import { InlineNotice, ScreenCard } from "../kit/surface";
import { KitButton } from "../kit/control";
import { SettingsRow, SettingsSection, SettingsSegmentedControl, SettingsSwitch, SettingsTextInput } from "../kit/settings";
import { errorPresentation, ErrorSurface } from "../states/error";
import { LoadingSurface, OfflineSurface } from "../states/surfaces";
import {
  NON_SUPPRESSIBLE_NOTIFICATION_CLASSES,
  NOTIFICATION_CHANNELS,
  normalizedNotificationClasses,
  withChannelEnabled,
  withClassEnabled,
  withDetailLevel,
  type NotificationChannel,
} from "./model";
import { usePrivacyMode } from "./privacy";

interface SettingsSnapshot {
  readonly profile: Profile;
  readonly notifications: NotificationPreferences;
  readonly binding: WalletBinding;
}

type LoadState =
  | Readonly<{ state: "loading" }>
  | Readonly<{ state: "offline" }>
  | Readonly<{ state: "error"; error: unknown }>
  | Readonly<{ state: "ready"; snapshot: SettingsSnapshot }>;

const CHANNEL_COPY_KEYS: Readonly<Record<NotificationChannel, string>> = {
  push: "settings.notifications.channel.push",
  email: "settings.notifications.channel.email",
  in_app: "settings.notifications.channel.in_app",
};

const CLASS_COPY_KEYS: Readonly<Record<NotificationClass, string>> = {
  "approval-waiting": "settings.notifications.class.approval_waiting",
  "money-arrived": "settings.notifications.class.money_arrived",
  "journey-finished": "settings.notifications.class.journey_finished",
  "claim-ready": "settings.notifications.class.claim_ready",
  "security-new-device": "settings.notifications.class.security_new_device",
  "security-recovery": "settings.notifications.class.security_recovery",
  "security-wallet-rebinding": "settings.notifications.class.security_wallet_rebinding",
  "security-key-rotation": "settings.notifications.class.security_key_rotation",
  "service-status": "settings.notifications.class.service_status",
};

const DETAIL_OPTIONS: readonly Readonly<{ value: NotificationDetailLevel; label: string }>[] = [
  { value: "minimal", label: copyEntry("settings.notifications.detail.minimal").message },
  { value: "summary", label: copyEntry("settings.notifications.detail.summary").message },
  { value: "full", label: copyEntry("settings.notifications.detail.full").message },
];

function bindingValue(binding: WalletBinding): string {
  if (binding.state === "bound" && binding.address !== undefined) {
    return binding.address;
  }
  return copyEntry(`settings.wallet.${binding.state}`).message;
}

function mutationFailure(error: unknown): string {
  if (
    typeof error === "object"
    && error !== null
    && "detail" in error
    && typeof error.detail === "object"
    && error.detail !== null
    && "code" in error.detail
    && error.detail.code === "not-suppressible"
  ) {
    return copyEntry("settings.notifications.non_suppressible").message;
  }
  return copyEntry("settings.save.failed").message;
}

function NotificationChannelEditor({
  channel,
  preferences,
  disabled,
  onChange,
  onRefused,
}: Readonly<{
  channel: NotificationChannel;
  preferences: NotificationPreferences;
  disabled: boolean;
  onChange: (candidate: NotificationPreferences) => void;
  onRefused: () => void;
}>) {
  const channelPreference = preferences[channel];
  const channelLabel = copyEntry(CHANNEL_COPY_KEYS[channel]).message;

  return (
    <SettingsSection title={channelLabel}>
      <SettingsRow
        title={copyEntry("settings.notifications.channel.enabled").message}
        subtitle={copyEntry("settings.notifications.channel.enabled.body").message}
        trailing={(
          <SettingsSwitch
            label={formatCopy("settings.notifications.channel.toggle", { channel: channelLabel })}
            checked={channelPreference.enabled}
            disabled={disabled}
            onCheckedChange={(enabled) => {
              const candidate = withChannelEnabled(preferences, channel, enabled);
              if (candidate === undefined) {
                onRefused();
              } else {
                onChange(candidate);
              }
            }}
          />
        )}
      />
      {normalizedNotificationClasses(channelPreference).map((entry) => {
        const classLabel = copyEntry(CLASS_COPY_KEYS[entry.class]).message;
        const critical = NON_SUPPRESSIBLE_NOTIFICATION_CLASSES.has(entry.class);
        return (
          <SettingsRow
            key={entry.class}
            title={classLabel}
            subtitle={critical
              ? copyEntry("settings.notifications.non_suppressible.caption").message
              : undefined}
            trailing={(
              <SettingsSwitch
                label={formatCopy("settings.notifications.class.toggle", {
                  notification: classLabel,
                  channel: channelLabel,
                })}
                checked={entry.enabled}
                disabled={disabled || !channelPreference.enabled}
                onCheckedChange={(enabled) => {
                  const candidate = withClassEnabled(preferences, channel, entry.class, enabled);
                  if (candidate === undefined) {
                    onRefused();
                  } else {
                    onChange(candidate);
                  }
                }}
              />
            )}
          />
        );
      })}
    </SettingsSection>
  );
}

export function SettingsScreen({
  client: suppliedClient,
}: Readonly<{ client?: HumanApiClient }>) {
  const client = useMemo(() => suppliedClient ?? humanApi(), [suppliedClient]);
  const router = useRouter();
  const { masked, setMasked } = usePrivacyMode();
  const [loadState, setLoadState] = useState<LoadState>({ state: "loading" });
  const [savingProfile, setSavingProfile] = useState(false);
  const [savingNotifications, setSavingNotifications] = useState(false);
  const [notice, setNotice] = useState<string | undefined>(undefined);
  const [profileOpen, setProfileOpen] = useState(false);
  const [displayName, setDisplayName] = useState("");
  const [avatarUrl, setAvatarUrl] = useState("");

  const load = useCallback(async () => {
    setLoadState({ state: "loading" });
    setNotice(undefined);
    try {
      const [profile, notifications, binding] = await Promise.all([
        client.profileGet(),
        client.notificationPreferencesGet(),
        client.bindingStatus(),
      ]);
      setDisplayName(profile.display_name);
      setAvatarUrl(profile.avatar_url ?? "");
      setLoadState({ state: "ready", snapshot: { profile, notifications, binding } });
    } catch (error) {
      setLoadState(navigator.onLine ? { state: "error", error } : { state: "offline" });
    }
  }, [client]);

  useEffect(() => {
    void load();
  }, [load]);

  const snapshot = loadState.state === "ready" ? loadState.snapshot : undefined;
  const profileValue = snapshot?.profile.display_name ?? "";
  const privacyValue = copyEntry(masked ? "settings.privacy.on" : "settings.privacy.off").message;

  const saveNotifications = async (candidate: NotificationPreferences) => {
    if (snapshot === undefined || savingNotifications) {
      return;
    }
    setSavingNotifications(true);
    setNotice(undefined);
    try {
      const notifications = await client.notificationPreferencesSet(candidate);
      setLoadState({ state: "ready", snapshot: { ...snapshot, notifications } });
      setNotice(copyEntry("settings.save.saved").message);
    } catch (error) {
      setNotice(mutationFailure(error));
    } finally {
      setSavingNotifications(false);
    }
  };

  const saveProfile = async (event: SyntheticEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (snapshot === undefined || savingProfile || displayName.trim().length === 0) {
      return;
    }
    setSavingProfile(true);
    setNotice(undefined);
    try {
      const profile = await client.profileUpdate({
        display_name: displayName.trim(),
        avatar_url: avatarUrl.trim(),
      });
      setLoadState({ state: "ready", snapshot: { ...snapshot, profile } });
      setProfileOpen(false);
      setNotice(copyEntry("settings.save.saved").message);
    } catch (error) {
      setNotice(mutationFailure(error));
    } finally {
      setSavingProfile(false);
    }
  };

  if (loadState.state === "loading") {
    return <LoadingSurface rows={6} />;
  }
  if (loadState.state === "offline") {
    return <OfflineSurface onRetry={() => { void load(); }} />;
  }
  if (loadState.state === "error") {
    return (
      <ErrorSurface
        error={errorPresentation(loadState.error)}
        route="/app/settings"
        onRetry={() => { void load(); }}
        onReload={() => { window.location.reload(); }}
      />
    );
  }

  return (
    <ScreenCard
      title={copyEntry("settings.title").message}
      description={copyEntry("settings.summary").message}
      dataApplication="settings"
    >
      <div className="mt-4 flex flex-col gap-4">
        {notice === undefined ? null : (
          <InlineNotice
            tone={notice === copyEntry("settings.save.saved").message ? "success" : "warning"}
          >
            {notice}
          </InlineNotice>
        )}

        <SettingsSection title={copyEntry("settings.section.profile").message}>
          <SettingsRow
            title={copyEntry("settings.profile.display_name").message}
            subtitle={profileValue}
            trailing={copyEntry("settings.action.edit").message}
            navigates
            onClick={() => { setProfileOpen((current) => !current); }}
          />
          {profileOpen ? (
            <form className="flex flex-col gap-3 py-3" onSubmit={(event) => { void saveProfile(event); }}>
              <label className="flex flex-col gap-1 text-sm font-semibold text-foreground">
                {copyEntry("settings.profile.display_name").message}
                <SettingsTextInput
                  value={displayName}
                  maxLength={128}
                  autoComplete="name"
                  onChange={(event) => { setDisplayName(event.target.value); }}
                />
              </label>
              <label className="flex flex-col gap-1 text-sm font-semibold text-foreground">
                {copyEntry("settings.profile.avatar").message}
                <SettingsTextInput
                  value={avatarUrl}
                  type="url"
                  inputMode="url"
                  maxLength={2048}
                  autoComplete="url"
                  onChange={(event) => { setAvatarUrl(event.target.value); }}
                />
              </label>
              <KitButton
                type="submit"
                loading={savingProfile}
                {...(displayName.trim().length === 0
                  ? {
                      disabled: true as const,
                      disabledReason: copyEntry("settings.profile.name_required").message,
                    }
                  : {})}
              >
                {copyEntry("settings.action.save").message}
              </KitButton>
            </form>
          ) : null}
        </SettingsSection>

        <SettingsSection title={copyEntry("settings.section.security").message}>
          <SettingsRow
            title={copyEntry("settings.security.title").message}
            subtitle={copyEntry("settings.security.current").message}
            navigates
            onClick={() => { router.push("/app/settings/security"); }}
          />
        </SettingsSection>

        <SettingsSection title={copyEntry("settings.section.wallet").message}>
          <SettingsRow
            title={copyEntry("settings.wallet.title").message}
            subtitle={bindingValue(loadState.snapshot.binding)}
            navigates
            onClick={() => { router.push("/app/settings/wallet"); }}
          />
        </SettingsSection>

        <SettingsSection title={copyEntry("settings.section.notifications").message}>
          <SettingsRow
            title={copyEntry("settings.notifications.detail.title").message}
            subtitle={copyEntry("settings.notifications.detail.body").message}
            trailing={copyEntry(`settings.notifications.detail.${loadState.snapshot.notifications.detail}`).message}
          />
          <div className="py-3">
            <SettingsSegmentedControl
              aria-label={copyEntry("settings.notifications.detail.title").message}
              value={loadState.snapshot.notifications.detail}
              options={DETAIL_OPTIONS.map((option) => ({ ...option }))}
              onValueChange={(value) => {
                void saveNotifications(withDetailLevel(
                  loadState.snapshot.notifications,
                  value as NotificationDetailLevel,
                ));
              }}
            />
          </div>
        </SettingsSection>

        {NOTIFICATION_CHANNELS.map((channel) => (
          <NotificationChannelEditor
            key={channel}
            channel={channel}
            preferences={loadState.snapshot.notifications}
            disabled={savingNotifications}
            onChange={(candidate) => { void saveNotifications(candidate); }}
            onRefused={() => { setNotice(copyEntry("settings.notifications.non_suppressible").message); }}
          />
        ))}

        <SettingsSection title={copyEntry("settings.section.advanced").message}>
          <SettingsRow
            title={copyEntry("settings.privacy.title").message}
            subtitle={copyEntry("settings.privacy.body").message}
            trailing={(
              <SettingsSwitch
                label={copyEntry("settings.privacy.toggle").message}
                checked={masked}
                onCheckedChange={setMasked}
              />
            )}
            trailingCaption={privacyValue}
          />
          <SettingsRow
            title={copyEntry("exit.title").message}
            subtitle={copyEntry("exit.summary").message}
            navigates
            onClick={() => { router.push("/app/settings/exit"); }}
          />
        </SettingsSection>

        <SettingsSection title={copyEntry("settings.section.help").message}>
          <SettingsRow
            title={copyEntry("settings.help.support").message}
            subtitle={copyEntry("settings.help.support.body").message}
            navigates
            onClick={() => { router.push("/app/support"); }}
          />
        </SettingsSection>
      </div>
    </ScreenCard>
  );
}
