"use client";

import { AmountText, type AmountTextProps } from "@layerx/ui/components/amount";
import { Badge } from "@layerx/ui/components/badge";
import { Button } from "@layerx/ui/components/button";
import { Card } from "@layerx/ui/components/card";
import { DetailDisclosure } from "@layerx/ui/components/detail";
import { Input } from "@layerx/ui/components/input";
import { cn } from "@layerx/ui/cn";
import { type PlatformSetting } from "@layerx/ui/lib/platform";
import { useId, useState, type ReactNode } from "react";

import { copyEntry } from "../../copy/catalog.ts";
import { formatExplicitCurrencyAmount, LiveRegion } from "./a11y";
import {
  directionWord,
  feeMathTotal,
  statusPresentation,
  type MoneyDirection,
  type ProtocolAmount,
  type StatusKey,
} from "./model";

export type SignedWordedAmountProps = Omit<
  AmountTextProps,
  "colorMode" | "currency" | "locale" | "value"
> &
  Readonly<{
    value: ProtocolAmount;
    readonly currency: string;
    readonly locale: string;
    readonly direction?: MoneyDirection;
  }>;

export function SignedWordedAmount({
  value,
  currency,
  locale,
  decimals,
  direction,
  className,
  ...props
}: SignedWordedAmountProps) {
  const resolvedDirection = direction ?? (value > 0 ? "inbound" : value < 0 ? "outbound" : "other");
  const amountLabel = formatExplicitCurrencyAmount(value, {
    currency,
    locale,
    ...(decimals === undefined ? {} : { decimals }),
  });
  return (
    <span className={cn("inline-flex items-baseline gap-2", className)}>
      <span className="sr-only">{`${amountLabel}, ${directionWord(resolvedDirection)}`}</span>
      <AmountText
        {...props}
        aria-hidden
        value={value}
        currency={currency}
        locale={locale}
        {...(decimals === undefined ? {} : { decimals })}
        colorMode="signed"
      />
      <span aria-hidden className="text-sm font-medium text-foreground-secondary">
        {directionWord(resolvedDirection)}
      </span>
    </span>
  );
}

export function LabelValue({
  label,
  value,
  className,
}: Readonly<{ label: ReactNode; value: ReactNode; className?: string }>) {
  return (
    <span className={cn("inline-flex flex-col gap-1", className)}>
      <span className="text-sm text-muted-foreground">{label}</span>
      <span className="font-semibold tabular-nums text-foreground">{value}</span>
    </span>
  );
}

export function StatusPill({ status, className }: Readonly<{ status: StatusKey; className?: string }>) {
  const presentation = statusPresentation(status);
  return (
    <Badge
      variant={presentation.tone}
      className={className}
      role="status"
      aria-live="polite"
      aria-atomic="true"
    >
      {presentation.label}
    </Badge>
  );
}

export function CopyableIdentifier({
  label,
  value,
  onCopied,
  className,
}: Readonly<{ label: string; value: string; onCopied?: (value: string) => void; className?: string }>) {
  const [copyState, setCopyState] = useState<"copied" | "failed" | "idle">("idle");
  const labelId = useId();

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopyState("copied");
      onCopied?.(value);
    } catch {
      setCopyState("failed");
    }
  };

  const copyAnnouncement =
    copyState === "copied"
      ? copyEntry("action.copied").message
      : copyState === "failed"
        ? copyEntry("action.copy_failed").message
        : undefined;

  return (
    <div className={cn("flex flex-col gap-2 text-sm font-semibold text-foreground", className)}>
      <span id={labelId}>{label}</span>
      <Input
        aria-labelledby={labelId}
        readOnly
        value={value}
        className="tabular-nums"
        trailing={
          <Button type="button" variant="link" size="sm" onClick={() => void copy()}>
            {copyEntry(copyState === "copied" ? "action.copied" : "action.copy").message}
          </Button>
        }
      />
      <LiveRegion priority={copyState === "failed" ? "assertive" : "polite"}>
        {copyAnnouncement}
      </LiveRegion>
    </div>
  );
}

export interface FeeMathStep {
  readonly label: string;
  readonly amount: ProtocolAmount;
  readonly explanation?: string;
}

export interface FeeMathDisclosureProps {
  readonly open: boolean;
  readonly onOpenChange: (open: boolean) => void;
  readonly steps: readonly [FeeMathStep, ...FeeMathStep[]];
  readonly title?: string;
  readonly totalLabel?: string;
  readonly currency: string;
  readonly locale: string;
  readonly symbol?: string;
  readonly decimals?: number;
  readonly platform: PlatformSetting;
  readonly portalContainer?: HTMLElement | null;
}

export function FeeMathDisclosure({
  open,
  onOpenChange,
  steps,
  title = copyEntry("fee.disclosure.title").message,
  totalLabel = copyEntry("fee.disclosure.total").message,
  currency,
  locale,
  symbol,
  decimals,
  platform,
  portalContainer,
}: FeeMathDisclosureProps) {
  const amountProps = {
    currency,
    locale,
    ...(symbol === undefined ? {} : { symbol }),
    ...(decimals === undefined ? {} : { decimals }),
  };
  const total = feeMathTotal(steps);

  return (
    <DetailDisclosure
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      summary={title}
      desktopVariant="inline"
      platform={platform}
      {...(portalContainer === undefined ? {} : { portalContainer })}
    >
      <ol className="flex flex-col gap-3">
        {steps.map((step, index) => (
          <li key={`${String(index)}-${step.label}`} className="flex items-start gap-3">
            <span className="inline-flex size-7 shrink-0 items-center justify-center rounded-full bg-surface-sunken text-sm font-bold tabular-nums text-foreground">
              {index + 1}
            </span>
            <Card elevation="outline" padding="sm" className="flex min-w-0 flex-1 items-start justify-between gap-4">
              <span className="flex flex-col gap-1">
                <span className="font-semibold text-foreground">{step.label}</span>
                {step.explanation === undefined ? null : (
                  <span className="text-sm text-muted-foreground">{step.explanation}</span>
                )}
              </span>
              <span>
                <span className="sr-only">
                  {`${step.label}: ${formatExplicitCurrencyAmount(step.amount, {
                    currency,
                    locale,
                    ...(decimals === undefined ? {} : { decimals }),
                  })}`}
                </span>
                <AmountText
                  {...amountProps}
                  aria-hidden
                  value={step.amount}
                  colorMode="neutral"
                />
              </span>
            </Card>
          </li>
        ))}
      </ol>
      <div className="mt-4 flex items-center justify-between border-t border-border pt-4">
        <span className="font-semibold text-foreground">{totalLabel}</span>
        <span>
          <span className="sr-only">
            {`${totalLabel}: ${formatExplicitCurrencyAmount(total, {
              currency,
              locale,
              ...(decimals === undefined ? {} : { decimals }),
            })}`}
          </span>
          <AmountText {...amountProps} aria-hidden value={total} colorMode="neutral" />
        </span>
      </div>
    </DetailDisclosure>
  );
}
