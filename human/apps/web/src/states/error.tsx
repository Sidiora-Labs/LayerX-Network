"use client";

import {
  Component,
  useEffect,
  useMemo,
  useState,
  type ErrorInfo,
  type ReactNode,
} from "react";

import { copyEntry } from "../../copy/runtime.ts";
import { formatCopy } from "../../copy/format.ts";
import { DesktopConfirmation, MobileConfirmation } from "../kit/confirm";
import { DesktopDetail, MobileDetail } from "../kit/pattern-detail";
import { InlineNotice, StateFrame } from "../kit/surface";
import { KitButton } from "../kit/control";
import {
  browserSupportReportOutbox,
  type ErrorReporter,
} from "./report";

export interface ErrorPresentation {
  readonly titleKey: string;
  readonly descriptionKey: string;
  readonly moneyImpactKey: string;
  readonly machineCode: string;
  readonly traceId: string;
  readonly retriable: boolean;
  readonly structural: boolean;
}

function record(value: unknown): Readonly<Record<string, unknown>> | undefined {
  return typeof value === "object" && value !== null
    ? value as Readonly<Record<string, unknown>>
    : undefined;
}

function diagnostic(value: unknown, key: string): string | undefined {
  const candidate = record(value)?.[key];
  return typeof candidate === "string" && candidate.length > 0 ? candidate : undefined;
}

function generatedTraceId(): string {
  const suffix = typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
    ? crypto.randomUUID().replaceAll("-", "")
    : Date.now().toString(16).padStart(32, "0").slice(-32);
  return `trc_${suffix}`;
}

export function errorPresentation(error: unknown, traceId = generatedTraceId()): ErrorPresentation {
  const detail = record(record(error)?.detail);
  const suppliedMachineCode = diagnostic(detail, "code") ?? diagnostic(error, "code");
  const machineCode = suppliedMachineCode !== undefined
    && suppliedMachineCode.length <= 120
    && /^[A-Z][A-Z0-9._:-]*$/u.test(suppliedMachineCode)
    ? suppliedMachineCode
    : "UI_UNHANDLED";
  const suppliedTrace = diagnostic(detail, "trace_id")
    ?? diagnostic(error, "traceId")
    ?? diagnostic(error, "trace")
    ?? diagnostic(error, "digest");
  const canonicalTrace = suppliedTrace !== undefined && /^trc_[0-9a-f]{32}$/u.test(suppliedTrace)
    ? suppliedTrace
    : undefined;
  return Object.freeze({
    titleKey: "state.error",
    descriptionKey: "state.error.body",
    moneyImpactKey: "error.money.not_started",
    machineCode,
    traceId: canonicalTrace ?? traceId,
    retriable: true,
    structural: true,
  });
}

function useResolvedPlatform(platform?: "mobile" | "desktop"): "mobile" | "desktop" {
  const [resolved, setResolved] = useState<"mobile" | "desktop">(platform ?? "desktop");
  useEffect(() => {
    if (platform !== undefined) {
      setResolved(platform);
      return;
    }
    const update = () => {
      const shell = document.querySelector<HTMLElement>("[data-shell]")?.dataset.shell;
      setResolved(shell === "mobile" || shell === "desktop"
        ? shell
        : window.matchMedia("(max-width: 767px)").matches ? "mobile" : "desktop");
    };
    update();
    const media = window.matchMedia("(max-width: 767px)");
    media.addEventListener("change", update);
    return () => { media.removeEventListener("change", update); };
  }, [platform]);
  return resolved;
}

export function ErrorSurface({
  error,
  route,
  platform,
  onRetry,
  onReload,
  reporter = browserSupportReportOutbox,
}: Readonly<{
  error: ErrorPresentation;
  route: string;
  platform?: "mobile" | "desktop";
  onRetry?: () => void;
  onReload?: () => void;
  reporter?: ErrorReporter;
}>) {
  const resolvedPlatform = useResolvedPlatform(platform);
  const [technicalOpen, setTechnicalOpen] = useState(false);
  const [reportOpen, setReportOpen] = useState(false);
  const [reportStatus, setReportStatus] = useState<"idle" | "saving" | "saved" | "pending" | "failed">("idle");
  const Confirmation = resolvedPlatform === "mobile" ? MobileConfirmation : DesktopConfirmation;
  const Detail = resolvedPlatform === "mobile" ? MobileDetail : DesktopDetail;
  const technicalDetails = useMemo(() => (
    <div className="flex flex-col gap-2 font-mono text-xs text-foreground-secondary">
      <p>{formatCopy("error.technical.machine_code", { machineCode: error.machineCode })}</p>
      <p>{formatCopy("error.technical.trace", { traceId: error.traceId })}</p>
    </div>
  ), [error.machineCode, error.traceId]);

  const submitReport = async () => {
    setReportStatus("saving");
    try {
      const receipt = await reporter.submit({
        consented: true,
        traceId: error.traceId,
        machineCode: error.machineCode,
        route,
        shell: resolvedPlatform,
      });
      setReportStatus(receipt.status);
      setReportOpen(false);
    } catch {
      setReportStatus("failed");
      setReportOpen(false);
    }
  };

  return (
    <StateFrame
      title={copyEntry(error.titleKey).message}
      description={copyEntry(error.descriptionKey).message}
      tone="danger"
      role="alert"
    >
      <p className="text-sm font-semibold">{copyEntry(error.moneyImpactKey).message}</p>
      <div className="flex flex-wrap gap-2">
        {error.retriable && onRetry !== undefined ? (
          <KitButton variant="primary" onClick={onRetry}>{copyEntry("action.retry").message}</KitButton>
        ) : null}
        {error.structural && onReload !== undefined ? (
          <KitButton variant="secondary" onClick={onReload}>{copyEntry("action.reload").message}</KitButton>
        ) : null}
        <KitButton variant="secondary" onClick={() => { setReportOpen(true); }}>
          {copyEntry("action.report").message}
        </KitButton>
        <KitButton
          variant="secondary"
          onClick={() => { window.location.assign(`/app/support?trace=${encodeURIComponent(error.traceId)}`); }}
        >
          {copyEntry("support.open").message}
        </KitButton>
      </div>
      {reportStatus === "saved" ? (
        <InlineNotice tone="success" role="status">
          <span className="font-semibold">{copyEntry("report.saved").message}</span>{" "}
          {copyEntry("report.saved.body").message}
        </InlineNotice>
      ) : null}
      {reportStatus === "pending" ? (
        <InlineNotice tone="warning" role="status">
          <span className="font-semibold">{copyEntry("report.pending").message}</span>{" "}
          {copyEntry("report.pending.body").message}
        </InlineNotice>
      ) : null}
      {reportStatus === "failed" ? (
        <InlineNotice tone="danger" role="alert">{copyEntry("report.failed").message}</InlineNotice>
      ) : null}
      <Detail
        open={technicalOpen}
        onOpenChange={setTechnicalOpen}
        title={copyEntry("error.technical.title").message}
        summary={copyEntry("error.technical.title").message}
        mobileVariant="sheet"
        desktopVariant="inline"
      >
        {technicalDetails}
      </Detail>
      <Confirmation
        open={reportOpen}
        onOpenChange={setReportOpen}
        kind="reversible"
        title={copyEntry("report.consent.title").message}
        consequence={copyEntry("report.consent.body").message}
        confirmLabel={copyEntry("report.submit").message}
        loading={reportStatus === "saving"}
        onConfirm={() => { void submitReport(); }}
      />
    </StateFrame>
  );
}

interface ErrorBoundaryProps {
  readonly children: ReactNode;
  readonly route: string;
  readonly platform?: "mobile" | "desktop";
  readonly reporter?: ErrorReporter;
  readonly onReset?: () => void;
  readonly onError?: (error: Error, information: ErrorInfo) => void;
}

interface ErrorBoundaryState {
  readonly error: Error | undefined;
  readonly traceId: string | undefined;
}

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: undefined, traceId: undefined };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error, traceId: generatedTraceId() };
  }

  componentDidCatch(error: Error, information: ErrorInfo): void {
    this.props.onError?.(error, information);
  }

  componentDidUpdate(previous: ErrorBoundaryProps): void {
    if (previous.route !== this.props.route && this.state.error !== undefined) {
      this.setState({ error: undefined, traceId: undefined });
    }
  }

  reset = (): void => {
    this.setState({ error: undefined, traceId: undefined });
    this.props.onReset?.();
  };

  render(): ReactNode {
    if (this.state.error === undefined) {
      return this.props.children;
    }
    return (
      <ErrorSurface
        error={errorPresentation(this.state.error, this.state.traceId)}
        route={this.props.route}
        {...(this.props.platform === undefined ? {} : { platform: this.props.platform })}
        {...(this.props.reporter === undefined ? {} : { reporter: this.props.reporter })}
        onRetry={this.reset}
        onReload={() => { window.location.reload(); }}
      />
    );
  }
}

export function RouteErrorBoundary({
  error,
  reset,
  route,
}: Readonly<{ error: Error & { digest?: string }; reset: () => void; route: string }>) {
  return (
    <ErrorSurface
      error={errorPresentation(error)}
      route={route}
      onRetry={reset}
      onReload={() => { window.location.reload(); }}
    />
  );
}
