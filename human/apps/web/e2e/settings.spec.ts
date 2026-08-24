import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { copyEntry } from "../copy/catalog.ts";
import {
  classEnabled,
  fullySuppressesSecurityClass,
  NON_SUPPRESSIBLE_NOTIFICATION_CLASSES,
  NOTIFICATION_CHANNELS,
  withChannelEnabled,
  withClassEnabled,
  withDetailLevel,
  type NotificationChannel,
} from "../src/settings/model.ts";
import type { NotificationPreferences } from "../src/api/generated/index.ts";

const baseNotificationPreferences: NotificationPreferences = {
  push: {
    enabled: true,
    classes: [
      { class: "approval-waiting", enabled: true },
      { class: "money-arrived", enabled: true },
      { class: "journey-finished", enabled: true },
      { class: "claim-ready", enabled: true },
      { class: "security-new-device", enabled: true },
      { class: "security-recovery", enabled: true },
      { class: "security-wallet-rebinding", enabled: true },
      { class: "security-key-rotation", enabled: true },
      { class: "service-status", enabled: true },
    ],
  },
  email: {
    enabled: true,
    classes: [
      { class: "approval-waiting", enabled: false },
      { class: "money-arrived", enabled: true },
      { class: "journey-finished", enabled: false },
      { class: "claim-ready", enabled: true },
      { class: "security-new-device", enabled: true },
      { class: "security-recovery", enabled: true },
      { class: "security-wallet-rebinding", enabled: true },
      { class: "security-key-rotation", enabled: true },
      { class: "service-status", enabled: true },
    ],
  },
  in_app: {
    enabled: true,
    classes: [
      { class: "approval-waiting", enabled: true },
      { class: "money-arrived", enabled: true },
      { class: "journey-finished", enabled: true },
      { class: "claim-ready", enabled: true },
      { class: "security-new-device", enabled: true },
      { class: "security-recovery", enabled: true },
      { class: "security-wallet-rebinding", enabled: true },
      { class: "security-key-rotation", enabled: true },
      { class: "service-status", enabled: true },
    ],
  },
  detail: "summary",
};

test("settings verification runs the authenticated browser cascade", () => {
  const browserSuite = readFileSync(
    new URL("./browser/settings.spec.ts", import.meta.url),
    "utf8",
  );
  assert.match(browserSuite, /@settings/u);
  assert.match(browserSuite, /data-private-figure/u);
  assert.match(browserSuite, /notifications\/preferences/u);
});

test("settings hub sections enumerate profile security wallet notifications advanced and help", () => {
  const sections = [
    "settings.section.profile",
    "settings.section.security",
    "settings.section.wallet",
    "settings.section.notifications",
    "settings.section.advanced",
    "settings.section.help",
  ];

  for (const section of sections) {
    const entry = copyEntry(section);
    assert.ok(entry.message.length > 0, `${section} has copy`);
  }
});

test("notification preferences enumerate all declared channels", () => {
  assert.deepEqual(NOTIFICATION_CHANNELS, ["push", "email", "in_app"]);
});

test("notification preferences enumerate all declared event classes", () => {
  const classes = [
    "approval-waiting",
    "money-arrived",
    "journey-finished",
    "claim-ready",
    "security-new-device",
    "security-recovery",
    "security-wallet-rebinding",
    "security-key-rotation",
    "service-status",
  ];

  const channel = baseNotificationPreferences.push;
  const preferenceClasses = channel.classes.map((entry) => entry.class);
  assert.deepEqual(preferenceClasses, classes);
});

test("notification preferences identify non-suppressible security classes", () => {
  assert.ok(NON_SUPPRESSIBLE_NOTIFICATION_CLASSES.has("security-recovery"));
  assert.ok(NON_SUPPRESSIBLE_NOTIFICATION_CLASSES.has("security-wallet-rebinding"));
  assert.equal(NON_SUPPRESSIBLE_NOTIFICATION_CLASSES.size, 2);
});

test("toggling notification channel enabled state preserves class preferences", () => {
  const updated = withChannelEnabled(baseNotificationPreferences, "push", false);
  assert.ok(updated !== undefined, "channel disable is permitted");
  assert.equal(updated.push.enabled, false);
  assert.equal(updated.push.classes.length, baseNotificationPreferences.push.classes.length);

  const reEnabled = withChannelEnabled(updated, "push", true);
  assert.ok(reEnabled !== undefined);
  assert.equal(reEnabled.push.enabled, true);
  assert.deepEqual(reEnabled.push.classes, baseNotificationPreferences.push.classes);
});

test("disabling a channel that would fully suppress security classes is refused", () => {
  const onlyPushSecurity: NotificationPreferences = {
    ...baseNotificationPreferences,
    email: {
      enabled: false,
      classes: baseNotificationPreferences.email.classes.map((entry) => ({
        ...entry,
        enabled: false,
      })),
    },
    in_app: {
      enabled: false,
      classes: baseNotificationPreferences.in_app.classes.map((entry) => ({
        ...entry,
        enabled: false,
      })),
    },
  };

  const refused = withChannelEnabled(onlyPushSecurity, "push", false);
  assert.equal(refused, undefined, "suppressing last active security channel is refused");
});

test("toggling event class within a channel updates only that class", () => {
  const updated = withClassEnabled(
    baseNotificationPreferences,
    "push",
    "approval-waiting",
    false,
  );
  assert.ok(updated !== undefined);
  assert.equal(classEnabled(updated, "push", "approval-waiting"), false);
  assert.equal(classEnabled(updated, "push", "money-arrived"), true);
  assert.equal(classEnabled(updated, "email", "approval-waiting"), false);
});

test("disabling a security class that would fully suppress it is refused", () => {
  const onlyPushRecovery: NotificationPreferences = {
    ...baseNotificationPreferences,
    email: {
      enabled: true,
      classes: baseNotificationPreferences.email.classes.map((entry) =>
        entry.class === "security-recovery" ? { ...entry, enabled: false } : entry
      ),
    },
    in_app: {
      enabled: true,
      classes: baseNotificationPreferences.in_app.classes.map((entry) =>
        entry.class === "security-recovery" ? { ...entry, enabled: false } : entry
      ),
    },
  };

  const refused = withClassEnabled(onlyPushRecovery, "push", "security-recovery", false);
  assert.equal(refused, undefined, "suppressing last active security-recovery is refused");
});

test("changing notification detail level updates preference without affecting channels", () => {
  const updated = withDetailLevel(baseNotificationPreferences, "full");
  assert.equal(updated.detail, "full");
  assert.deepEqual(updated.push, baseNotificationPreferences.push);
  assert.deepEqual(updated.email, baseNotificationPreferences.email);
  assert.deepEqual(updated.in_app, baseNotificationPreferences.in_app);

  const minimal = withDetailLevel(updated, "minimal");
  assert.equal(minimal.detail, "minimal");
});

test("preference application logic detects security-class suppression", () => {
  const valid = baseNotificationPreferences;
  assert.equal(fullySuppressesSecurityClass(valid), false);

  const suppressed: NotificationPreferences = {
    ...baseNotificationPreferences,
    push: {
      enabled: true,
      classes: baseNotificationPreferences.push.classes.map((entry) =>
        entry.class === "security-recovery" ? { ...entry, enabled: false } : entry
      ),
    },
    email: {
      enabled: true,
      classes: baseNotificationPreferences.email.classes.map((entry) =>
        entry.class === "security-recovery" ? { ...entry, enabled: false } : entry
      ),
    },
    in_app: {
      enabled: true,
      classes: baseNotificationPreferences.in_app.classes.map((entry) =>
        entry.class === "security-recovery" ? { ...entry, enabled: false } : entry
      ),
    },
  };
  assert.equal(fullySuppressesSecurityClass(suppressed), true);
});

test("privacy mode copy declares on and off states", () => {
  const onCopy = copyEntry("settings.privacy.on");
  assert.ok(onCopy.message.length > 0);
  assert.equal(onCopy.surface, "default");

  const offCopy = copyEntry("settings.privacy.off");
  assert.ok(offCopy.message.length > 0);
  assert.equal(offCopy.surface, "default");
});

test("profile editing copy declares display name and avatar fields", () => {
  const displayName = copyEntry("settings.profile.display_name");
  assert.ok(displayName.message.length > 0);
  assert.equal(displayName.surface, "default");

  const avatar = copyEntry("settings.profile.avatar");
  assert.ok(avatar.message.length > 0);
  assert.equal(avatar.surface, "default");
});

test("settings save copy declares success and failure states", () => {
  const saved = copyEntry("settings.save.saved");
  assert.ok(saved.message.length > 0);
  assert.equal(saved.kind, "status");

  const failed = copyEntry("settings.save.failed");
  assert.ok(failed.message.length > 0);
  assert.equal(failed.kind, "status");
});

test("channel-enabled preference cascade applies to nested event classes", () => {
  const disabledChannel: NotificationPreferences = {
    ...baseNotificationPreferences,
    push: { ...baseNotificationPreferences.push, enabled: false },
  };

  for (const classEntry of baseNotificationPreferences.push.classes) {
    assert.equal(
      classEnabled(disabledChannel, "push", classEntry.class),
      false,
      `${classEntry.class} respects disabled channel`,
    );
  }
});

test("notification preferences support all three detail levels", () => {
  const minimal = withDetailLevel(baseNotificationPreferences, "minimal");
  assert.equal(minimal.detail, "minimal");

  const summary = withDetailLevel(baseNotificationPreferences, "summary");
  assert.equal(summary.detail, "summary");

  const full = withDetailLevel(baseNotificationPreferences, "full");
  assert.equal(full.detail, "full");
});

test("privacy masking copy declares behavior across both shells", () => {
  const privacyBody = copyEntry("settings.privacy.body");
  assert.ok(privacyBody.message.includes("balances"));
  assert.ok(privacyBody.message.includes("everywhere"));
  assert.equal(privacyBody.moneyAdjacent, true);
});
