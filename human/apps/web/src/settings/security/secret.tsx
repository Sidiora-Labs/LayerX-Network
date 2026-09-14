"use client";

import { useEffect, useState } from "react";

import { copyEntry } from "../../../copy/runtime";
import type { TimedSecret } from "../../api";
import { InlineNotice } from "../../kit/surface";
import { KitButton } from "../../kit/control";
import { secretExpiry } from "./model";

interface TimedSecretViewProps {
  readonly label: string;
  readonly secret: TimedSecret;
  readonly onExpired: () => void;
}

export function TimedSecretView({ label, secret, onExpired }: TimedSecretViewProps) {
  const [revealed, setRevealed] = useState(false);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");

  useEffect(() => {
    const remaining = Math.max(0, secretExpiry(secret) - Date.now());
    const timer = window.setTimeout(onExpired, Math.min(remaining, 2_147_483_647));
    return () => { window.clearTimeout(timer); };
  }, [onExpired, secret]);

  const copy = async () => {
    if (!secret.copyable || !revealed) {
      return;
    }
    try {
      await navigator.clipboard.writeText(secret.value);
      setCopyState("copied");
    } catch {
      setCopyState("failed");
    }
  };

  return (
    <div className="flex flex-col gap-2 py-2" data-timed-secret="">
      <span className="text-sm font-semibold text-foreground">{label}</span>
      <code className="break-all text-sm" aria-live="polite">
        {revealed ? secret.value : "•••• •••• ••••"}
      </code>
      <div className="flex flex-wrap gap-2">
        <KitButton type="button" variant="secondary" onClick={() => { setRevealed((value) => !value); }}>
          {copyEntry(revealed ? "security.secret.hide" : "security.secret.show").message}
        </KitButton>
        {secret.copyable ? (
          <KitButton
            type="button"
            variant="secondary"
            {...(revealed
              ? {}
              : {
                  disabled: true as const,
                  disabledReason: copyEntry("security.secret.hidden").message,
                })}
            onClick={() => { void copy(); }}
          >
            {copyEntry("security.secret.copy").message}
          </KitButton>
        ) : null}
      </div>
      {copyState === "idle" ? null : (
        <InlineNotice tone={copyState === "copied" ? "success" : "warning"}>
          {copyEntry(copyState === "copied" ? "action.copied" : "action.copy_failed").message}
        </InlineNotice>
      )}
    </div>
  );
}

interface BackupCodesViewProps {
  readonly codes: readonly string[];
  readonly remaskAt: string;
  readonly copyable: boolean;
  readonly onExpired: () => void;
}

export function BackupCodesView({ codes, remaskAt, copyable, onExpired }: BackupCodesViewProps) {
  const [revealed, setRevealed] = useState(false);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");

  useEffect(() => {
    const expiry = Date.parse(remaskAt);
    const remaining = Number.isFinite(expiry) ? Math.max(0, expiry - Date.now()) : 0;
    const timer = window.setTimeout(onExpired, Math.min(remaining, 2_147_483_647));
    return () => { window.clearTimeout(timer); };
  }, [onExpired, remaskAt]);

  const copy = async () => {
    if (!copyable || !revealed) {
      return;
    }
    try {
      await navigator.clipboard.writeText(codes.join("\n"));
      setCopyState("copied");
    } catch {
      setCopyState("failed");
    }
  };

  return (
    <div className="flex flex-col gap-2 py-2" data-backup-codes="">
      <span className="text-sm font-semibold text-foreground">
        {copyEntry("security.backup.title").message}
      </span>
      <span className="text-sm text-muted-foreground">
        {copyEntry("security.backup.body").message}
      </span>
      {revealed ? (
        <ul className="grid grid-cols-2 gap-2" aria-live="polite">
          {codes.map((code) => <li key={code}><code>{code}</code></li>)}
        </ul>
      ) : (
        <code aria-live="polite">•••• ••••</code>
      )}
      <div className="flex flex-wrap gap-2">
        <KitButton type="button" variant="secondary" onClick={() => { setRevealed((value) => !value); }}>
          {copyEntry(revealed ? "security.secret.hide" : "security.secret.show").message}
        </KitButton>
        {copyable ? (
          <KitButton
            type="button"
            variant="secondary"
            {...(revealed
              ? {}
              : {
                  disabled: true as const,
                  disabledReason: copyEntry("security.secret.hidden").message,
                })}
            onClick={() => { void copy(); }}
          >
            {copyEntry("security.secret.copy").message}
          </KitButton>
        ) : null}
      </div>
      {copyState === "idle" ? null : (
        <InlineNotice tone={copyState === "copied" ? "success" : "warning"}>
          {copyEntry(copyState === "copied" ? "action.copied" : "action.copy_failed").message}
        </InlineNotice>
      )}
    </div>
  );
}
