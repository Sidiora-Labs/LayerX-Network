"use client";

import { useState, type ReactNode } from "react";

import { copyEntry } from "../../../copy/runtime.ts";
import { formatCopy } from "../../../copy/format.ts";
import type {
  ActivityEntryDetail,
  ApprovalDetail,
  ApprovalState,
  ApprovalSummary,
  Timestamp,
} from "../../api/index.ts";
import { Badge, List, ListItem } from "../../kit/collection";
import { CopyableIdentifier, LabelValue, StatusPill } from "../../kit/money";
import { DesktopConfirmation, MobileConfirmation, type ConfirmationProps } from "../../kit/confirm";
import { DesktopDetail, MobileDetail } from "../../kit/patterns";
import { KitButton } from "../../kit/control";
import { ScreenCard, StateEmpty } from "../../kit/surface";
import { PrivateFigure } from "../../settings/privacy";
import type { Shell } from "../../shell/selector";
import {
  activityRoute,
  approvalInboxItem,
  approvalStatePresentation,
  approveConsequence,
  canApprove,
  catalogMessage,
  expiryCountdown,
  heldDigest,
  journeyStatus,
  moneyLabel,
  requestedAtLabel,
  verificationLabel,
  verifiedMoneyLabel,
  type ApprovalOutcome,
  type ApprovalTone,
} from "./model.ts";

const NOTICE_CLASS: Readonly<Record<ApprovalTone, string>> = Object.freeze({
  destructive: "border-destructive bg-destructive-soft text-destructive",
  neutral: "border-border bg-surface text-foreground",
  success: "border-success bg-success-soft text-success",
  warning: "border-warning bg-warning-soft text-warning",
});

function DecisionNotice({
  tone,
  children,
}: Readonly<{ tone: ApprovalTone; children: ReactNode }>) {
  return (
    <p
      role={tone === "destructive" ? "alert" : "status"}
      className={`rounded-md border px-4 py-3 text-sm font-medium ${NOTICE_CLASS[tone]}`}
    >
      {children}
    </p>
  );
}

export function ApprovalCountdown({
  expiresAt,
  at,
}: Readonly<{ expiresAt: Timestamp; at: Date }>) {
  const countdown = expiryCountdown(expiresAt, at);
  return (
    <span role="timer" aria-live="off" className="text-sm font-medium text-foreground-secondary">
      {countdown.label}
    </span>
  );
}

export function ApprovalStateBadge({
  state,
  stateCopyKey,
}: Readonly<{ state: ApprovalState; stateCopyKey?: string }>) {
  const presentation = approvalStatePresentation(state, stateCopyKey);
  return (
    <Badge variant={presentation.tone} size="sm">
      {presentation.label}
    </Badge>
  );
}

export function ApprovalInboxList({
  approvals,
  at,
  onOpen,
  selectedId,
}: Readonly<{
  approvals: readonly ApprovalSummary[];
  at: Date;
  onOpen?: ((approvalId: string) => void) | undefined;
  selectedId?: string | undefined;
}>) {
  if (approvals.length === 0) {
    return (
      <StateEmpty
        title={copyEntry("approval.inbox.title").message}
        description={copyEntry("approval.inbox.empty.body").message}
      />
    );
  }
  const budgetLabel = copyEntry("approval.detail.budget").message;
  return (
    <List>
      {approvals.map((summary) => {
        const item = approvalInboxItem(summary, at);
        return (
          <ListItem
            key={item.id}
            navigates
            aria-current={selectedId === item.id ? "true" : undefined}
            onClick={onOpen === undefined ? undefined : () => { onOpen(item.id); }}
            title={
              <span className="flex items-center gap-2">
                <span className="truncate">{item.counterparty}</span>
                <PrivateFigure className="tabular-nums">{item.amountLabel}</PrivateFigure>
              </span>
            }
            subtitle={
              <span className="flex flex-col gap-0.5 whitespace-normal">
                <span>{`${item.agentName} · ${item.reason}`}</span>
                <span className="inline-flex flex-wrap gap-1">
                  <span>{budgetLabel}</span>
                  <PrivateFigure>{item.budgetAfterLabel}</PrivateFigure>
                  <span>{`· ${item.budgetVerification}`}</span>
                </span>
              </span>
            }
            trailing={<ApprovalStateBadge state={item.state} />}
            trailingCaption={<ApprovalCountdown expiresAt={summary.expires_at} at={at} />}
          />
        );
      })}
    </List>
  );
}

export function MobileApprovalInbox({
  approvals,
  at,
  onOpen,
}: Readonly<{
  approvals: readonly ApprovalSummary[];
  at: Date;
  onOpen?: ((approvalId: string) => void) | undefined;
}>) {
  return (
    <ScreenCard
      dataApplication="approvals"
      title={copyEntry("approval.inbox.title").message}
      description={copyEntry("approval.inbox.description").message}
    >
      <ApprovalInboxList approvals={approvals} at={at} onOpen={onOpen} />
    </ScreenCard>
  );
}

export function DesktopApprovalSplit({
  approvals,
  at,
  onOpen,
  selectedId,
  children,
}: Readonly<{
  approvals: readonly ApprovalSummary[];
  at: Date;
  onOpen?: ((approvalId: string) => void) | undefined;
  selectedId?: string | undefined;
  children?: ReactNode;
}>) {
  return (
    <div data-application="approvals" className="grid gap-4 p-4 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
      <ScreenCard
        landmark="section"
        title={copyEntry("approval.inbox.title").message}
        description={copyEntry("approval.inbox.description").message}
      >
        <ApprovalInboxList approvals={approvals} at={at} onOpen={onOpen} selectedId={selectedId} />
      </ScreenCard>
      {children ?? (
        <ScreenCard landmark="section" title={copyEntry("approval.detail.title").message}>
          <p className="text-foreground-secondary">{copyEntry("approval.inbox.select").message}</p>
        </ScreenCard>
      )}
    </div>
  );
}

export function ReleasedActivitySection({ entry }: Readonly<{ entry?: ActivityEntryDetail | undefined }>) {
  return (
    <section className="flex flex-col gap-2">
      <h2 className="text-base font-bold text-foreground">
        {copyEntry("approval.released.title").message}
      </h2>
      {entry === undefined ? (
        <p className="text-sm text-muted-foreground">
          {copyEntry("approval.released.pending").message}
        </p>
      ) : (
        <div className="flex flex-col gap-3">
          <span className="flex items-center gap-3">
            <StatusPill status={journeyStatus(entry.state)} />
            <a className="text-sm font-semibold text-accent" href={activityRoute(entry.entry_id)}>
              {copyEntry("approval.released.track").message}
            </a>
          </span>
          <CopyableIdentifier
            label={copyEntry("approval.technical.activity_reference").message}
            value={entry.entry_id}
          />
          {entry.evidence.map((evidence) => (
            <CopyableIdentifier
              key={evidence.evidence_id}
              label={formatCopy("approval.technical.evidence_reference", {
                evidenceClass: evidence.class,
                verification: verificationLabel(evidence.verification),
              })}
              value={evidence.evidence_id}
            />
          ))}
        </div>
      )}
    </section>
  );
}

export function ApprovalOutcomePanel({
  outcome,
  released,
}: Readonly<{ outcome: ApprovalOutcome; released?: ActivityEntryDetail | undefined }>) {
  const tone: ApprovalTone =
    outcome.kind === "decided"
      ? outcome.decision.state === "approved" ? "success" : "neutral"
      : outcome.kind === "converged"
        ? outcome.detail.state === "approved" ? "success" : "neutral"
      : outcome.kind === "defective" ? "destructive" : "warning";
  return (
    <div className="flex flex-col gap-3">
      {outcome.kind === "already-decided" ? (
        <h2 className="text-base font-bold text-foreground">
          {copyEntry("approval.decided.title").message}
        </h2>
      ) : null}
      <DecisionNotice tone={tone}>{outcome.message}</DecisionNotice>
      {outcome.kind === "decided" ? outcome.decision.evidence.map((evidence) => (
        <CopyableIdentifier
          key={evidence.evidence_id}
          label={formatCopy("approval.technical.evidence_reference", {
            evidenceClass: evidence.class,
            verification: verificationLabel(evidence.verification),
          })}
          value={evidence.evidence_id}
        />
      )) : null}
      {(
        outcome.kind === "decided" && outcome.decision.state === "approved"
      ) || (
        outcome.kind === "converged" && outcome.detail.state === "approved"
      ) ? (
        <ReleasedActivitySection entry={released} />
      ) : null}
    </div>
  );
}

export function ApprovalDetailCard({
  detail,
  at,
  onApprove,
  onReject,
  outcome,
  released,
  deciding = false,
  shell,
}: Readonly<{
  detail: ApprovalDetail;
  at: Date;
  onApprove?: () => void;
  onReject?: () => void;
  outcome?: ApprovalOutcome | undefined;
  released?: ActivityEntryDetail | undefined;
  deciding?: boolean;
  shell: Shell;
}>) {
  const countdown = expiryCountdown(detail.facts.expires_at, at);
  const expired = detail.state === "expired" || (detail.state === "pending" && countdown.expired);
  const defective = detail.state === "defective"
    || (detail.state === "pending" && heldDigest(detail) === undefined);
  const decided = detail.state === "approved" || detail.state === "rejected";
  const approvable = canApprove(detail, at);
  const presentedState = expired ? "expired" : defective ? "defective" : detail.state;
  return (
    <ScreenCard
      dataApplication="approval-detail"
      landmark="section"
      title={copyEntry("approval.detail.title").message}
    >
      <div className="flex items-center gap-3">
        <ApprovalStateBadge
          state={presentedState}
          {...(presentedState === detail.state ? { stateCopyKey: detail.state_copy_key } : {})}
        />
        {detail.state === "pending" && !countdown.expired ? (
          <ApprovalCountdown expiresAt={detail.facts.expires_at} at={at} />
        ) : null}
      </div>
      <dl className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <LabelValue
          label={copyEntry("approval.detail.agent").message}
          value={detail.agent_name}
        />
        <LabelValue
          label={copyEntry("approval.detail.counterparty").message}
          value={detail.facts.counterparty}
        />
        <LabelValue
          label={copyEntry("approval.detail.amount").message}
          value={<PrivateFigure>{moneyLabel(detail.facts.amount)}</PrivateFigure>}
        />
        <LabelValue
          label={copyEntry("approval.detail.fees").message}
          value={<PrivateFigure>{moneyLabel(detail.facts.fees)}</PrivateFigure>}
        />
        <LabelValue
          label={copyEntry("approval.detail.requested").message}
          value={requestedAtLabel(detail.created_at)}
        />
        <LabelValue
          label={copyEntry("approval.detail.held_reason").message}
          value={catalogMessage(detail.reason_copy_key, "approval.detail.held_reason")}
        />
        <LabelValue
          label={copyEntry("approval.detail.budget").message}
          value={(
            <span className="inline-flex flex-wrap gap-1">
              <PrivateFigure>{verifiedMoneyLabel(detail.budget_remaining_after)}</PrivateFigure>
              <span>{`· ${verificationLabel(detail.budget_remaining_after.verification)}`}</span>
            </span>
          )}
        />
      </dl>
      <ApprovalEvidence detail={detail} shell={shell} />
      {outcome !== undefined ? (
        <ApprovalOutcomePanel outcome={outcome} released={released} />
      ) : decided ? (
        <div className="flex flex-col gap-3">
          <h2 className="text-base font-bold text-foreground">
            {copyEntry("approval.decided.title").message}
          </h2>
          <DecisionNotice tone={detail.state === "approved" ? "success" : "neutral"}>
            {catalogMessage(
              detail.state === "approved"
                ? "approval.approve.released"
                : "approval.reject.nothing-moved",
              detail.state_copy_key,
            )}
          </DecisionNotice>
          {detail.state === "approved" ? <ReleasedActivitySection entry={released} /> : null}
        </div>
      ) : expired ? (
        <DecisionNotice tone="warning">
          {copyEntry("error.approval.hold-expired").message}
        </DecisionNotice>
      ) : defective ? (
        <DecisionNotice tone="destructive">
          {copyEntry("error.approval.hold-defective").message}
        </DecisionNotice>
      ) : null}
      {approvable && outcome === undefined ? (
        <div className="flex flex-col gap-3 sm:flex-row">
          {deciding ? (
            <KitButton variant="primary" size="lg" loading>
              {copyEntry("approval.approve.action").message}
            </KitButton>
          ) : (
            <KitButton variant="primary" size="lg" onClick={onApprove}>
              {copyEntry("approval.approve.action").message}
            </KitButton>
          )}
          <KitButton variant="secondary" size="lg" onClick={onReject}>
            {copyEntry("approval.reject.action").message}
          </KitButton>
        </div>
      ) : null}
    </ScreenCard>
  );
}

function ApprovalEvidence({ detail, shell }: Readonly<{ detail: ApprovalDetail; shell: Shell }>) {
  const [open, setOpen] = useState(false);
  const Detail = shell === "mobile" ? MobileDetail : DesktopDetail;
  return (
    <Detail
      open={open}
      onOpenChange={setOpen}
      title={copyEntry("error.technical.title").message}
      summary={copyEntry("error.technical.title").message}
      desktopVariant="inline"
    >
      <div className="flex flex-col gap-3">
        <CopyableIdentifier
          label={copyEntry("approval.technical.approval_reference").message}
          value={detail.approval_id}
        />
        {detail.evidence.map((evidence) => (
          <CopyableIdentifier
            key={evidence.evidence_id}
            label={formatCopy("approval.technical.evidence_reference", {
              evidenceClass: evidence.class,
              verification: verificationLabel(evidence.verification),
            })}
            value={evidence.evidence_id}
          />
        ))}
      </div>
    </Detail>
  );
}

export function ApprovalDecisionConfirmations({
  detail,
  shell,
  approveOpen,
  rejectOpen,
  onApproveOpenChange,
  onRejectOpenChange,
  onConfirmApprove,
  onConfirmReject,
  deciding = false,
}: Readonly<{
  detail: ApprovalDetail;
  shell: Shell;
  approveOpen: boolean;
  rejectOpen: boolean;
  onApproveOpenChange: (open: boolean) => void;
  onRejectOpenChange: (open: boolean) => void;
  onConfirmApprove: () => void;
  onConfirmReject: () => void;
  deciding?: boolean;
}>) {
  const Confirmation = shell === "mobile" ? MobileConfirmation : DesktopConfirmation;
  const approve: ConfirmationProps = {
    kind: "destructive",
    open: approveOpen,
    onOpenChange: onApproveOpenChange,
    title: copyEntry("approval.approve.confirm_title").message,
    consequence: (
      <span className="flex flex-col gap-2">
        <PrivateFigure>{approveConsequence(detail)}</PrivateFigure>
        <span>{copyEntry("approval.approve.ceremony").message}</span>
      </span>
    ),
    confirmLabel: copyEntry("approval.approve.action").message,
    onConfirm: onConfirmApprove,
    loading: deciding,
  };
  const reject: ConfirmationProps = {
    kind: "destructive",
    open: rejectOpen,
    onOpenChange: onRejectOpenChange,
    title: copyEntry("approval.reject.confirm_title").message,
    consequence: copyEntry("approval.reject.consequence").message,
    confirmLabel: copyEntry("approval.reject.action").message,
    onConfirm: onConfirmReject,
    loading: deciding,
  };
  return (
    <>
      <Confirmation {...approve} />
      <Confirmation {...reject} />
    </>
  );
}
