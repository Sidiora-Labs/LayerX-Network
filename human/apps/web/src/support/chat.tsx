"use client";

import {
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

import { copyEntry } from "../../copy/runtime.ts";
import { formatCopy } from "../../copy/format.ts";
import type { TraceId } from "../api";
import { InlineNotice, ScreenCard, StateEmpty } from "../kit/surface";
import { KitButton } from "../kit/control";
import { KitTextField } from "../kit/display";
import { LiveRegion } from "../kit/a11y";
import { OfflineSurface } from "../states";
import {
  SupportChatSession,
  type SupportChatEntry,
  type SupportChatSnapshot,
} from "./conversation.ts";
import { SUPPORT_TOPICS } from "./topics.ts";

function useResolvedPlatform(platform: "mobile" | "desktop"): "mobile" | "desktop" {
  const [resolved, setResolved] = useState<"mobile" | "desktop">(platform);
  useEffect(() => {
    const shell = document.querySelector<HTMLElement>("[data-shell]")?.dataset.shell;
    if (shell === "mobile" || shell === "desktop") {
      setResolved(shell);
    }
  }, [platform]);
  return resolved;
}

function TopicSuggestions({ session }: Readonly<{ session: SupportChatSession }>) {
  return (
    <div
      role="group"
      aria-label={copyEntry("support.suggestions").message}
      className="flex flex-wrap justify-center gap-2"
    >
      {SUPPORT_TOPICS.map((topic) => (
        <KitButton
          key={topic.id}
          variant="soft"
          size="sm"
          onClick={() => {
            session.seedTopic(topic.id);
          }}
        >
          {copyEntry(topic.labelKey).message}
        </KitButton>
      ))}
    </div>
  );
}

function EntryBubble({
  entry,
  session,
  snapshot,
}: Readonly<{
  entry: SupportChatEntry;
  session: SupportChatSession;
  snapshot: SupportChatSnapshot;
}>) {
  const authorKey = entry.author === "you" ? "support.author.you" : "support.author.support";
  const bubbleClass = entry.author === "you"
    ? "self-end max-w-[85%] rounded-xl bg-surface-sunken px-4 py-2"
    : "self-start max-w-[85%] rounded-xl border border-border bg-surface px-4 py-2";
  return (
    <li className="flex flex-col gap-2">
      <div className={bubbleClass}>
        <p className="sr-only">{copyEntry(authorKey).message}</p>
        <p className="whitespace-pre-wrap text-sm leading-relaxed text-foreground">
          {entry.body}
        </p>
        {entry.delivery === "sending" ? (
          <p className="text-xs text-foreground-secondary">{copyEntry("support.message.sending").message}</p>
        ) : null}
      </div>
      {entry.delivery === "failed" ? (
        <InlineNotice tone="danger" role="alert">
          <span>{copyEntry("support.message.failed").message}</span>{" "}
          <KitButton
            variant="secondary"
            size="sm"
            onClick={() => {
              void session.retry(entry.message_id);
            }}
          >
            {copyEntry("action.retry").message}
          </KitButton>
        </InlineNotice>
      ) : null}
      {snapshot.awaitingFeedbackId === entry.message_id ? (
        <div className="flex flex-wrap items-center gap-2" data-feedback-state={snapshot.feedbackDelivery.get(entry.message_id) ?? "idle"}>
          <span className="text-sm text-foreground-secondary">
            {copyEntry("support.feedback.question").message}
          </span>
          <KitButton
            variant="secondary"
            size="sm"
            onClick={() => {
              void session.sendFeedback(entry.message_id, true);
            }}
          >
            {copyEntry("support.feedback.yes").message}
          </KitButton>
          <KitButton
            variant="secondary"
            size="sm"
            onClick={() => {
              void session.sendFeedback(entry.message_id, false);
            }}
          >
            {copyEntry("support.feedback.no").message}
          </KitButton>
          {snapshot.feedbackDelivery.get(entry.message_id) === "sending" ? (
            <span className="text-sm text-foreground-secondary">
              {copyEntry("support.feedback.sending").message}
            </span>
          ) : null}
          {snapshot.feedbackDelivery.get(entry.message_id) === "failed" ? (
            <InlineNotice tone="danger" role="alert">
              {copyEntry("support.feedback.failed").message}
            </InlineNotice>
          ) : null}
        </div>
      ) : null}
    </li>
  );
}

function announcement(snapshot: SupportChatSnapshot): string | undefined {
  if (snapshot.connection === "offline") {
    return copyEntry("state.offline.banner").message;
  }
  const last = snapshot.entries[snapshot.entries.length - 1];
  if (last === undefined) {
    return undefined;
  }
  if (last.author === "support") {
    return last.body;
  }
  if (last.delivery === "failed") {
    return copyEntry("support.message.failed").message;
  }
  return undefined;
}

export function SupportChat({
  platform,
  traceId,
}: Readonly<{ platform: "mobile" | "desktop"; traceId?: TraceId }>) {
  const resolved = useResolvedPlatform(platform);
  const sessionRef = useRef<SupportChatSession | undefined>(undefined);
  sessionRef.current ??= new SupportChatSession({
    shell: platform,
    ...(traceId === undefined ? {} : { traceId }),
  });
  const session = sessionRef.current;
  const snapshot = useSyncExternalStore(session.subscribe, session.snapshot, session.snapshot);
  const endRef = useRef<HTMLLIElement | null>(null);

  useEffect(() => {
    void session.initialize();
    const online = () => {
      session.setOnline(true);
      void session.retryFailed();
    };
    const offline = () => { session.setOnline(false); };
    window.addEventListener("online", online);
    window.addEventListener("offline", offline);
    if (!navigator.onLine) {
      offline();
    }
    return () => {
      window.removeEventListener("online", online);
      window.removeEventListener("offline", offline);
    };
  }, [session]);

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [snapshot.entries.length]);

  const draftEmpty = snapshot.draft.trim().length === 0;
  const containerClass = resolved === "mobile"
    ? "flex min-h-dvh flex-col"
    : "fixed bottom-6 right-6 z-40 flex max-h-[calc(100dvh-6rem)] w-96 max-w-[calc(100vw-3rem)] flex-col";

  const composer = (
    <form
      className="flex items-end gap-2"
      onSubmit={(event) => {
        event.preventDefault();
        void session.send();
      }}
    >
      <KitTextField
        label={copyEntry("support.compose.label").message}
        className="flex-1"
        maxLength={2000}
        value={snapshot.draft}
        onChange={(event) => {
          session.setDraft(event.target.value);
        }}
      />
      {draftEmpty || snapshot.sending ? (
        <KitButton
          type="submit"
          variant="primary"
          disabled
          disabledReason={copyEntry(snapshot.sending ? "support.message.sending" : "support.compose.required").message}
        >
          {copyEntry("support.send").message}
        </KitButton>
      ) : (
        <KitButton type="submit" variant="primary">
          {copyEntry("support.send").message}
        </KitButton>
      )}
    </form>
  );

  return (
    <div className={containerClass} data-support-shell={resolved}>
      <ScreenCard
        landmark="section"
        dataApplication="support"
        title={copyEntry("support.title").message}
        description={copyEntry("support.summary").message}
      >
        <LiveRegion>{announcement(snapshot)}</LiveRegion>
        {snapshot.traceId === undefined ? null : (
          <InlineNotice tone="neutral" role="status">
            <span className="font-semibold">{copyEntry("support.trace.attached").message}</span>{" "}
            <span className="font-mono text-xs">
              {formatCopy("error.technical.trace", { traceId: snapshot.traceId })}
            </span>
          </InlineNotice>
        )}
        {snapshot.connection === "offline" && snapshot.entries.length === 0 ? (
          <OfflineSurface
            onRetry={() => {
              void session.retryFailed();
            }}
          />
        ) : snapshot.load === "loading" ? (
          <InlineNotice tone="neutral" role="status">
            {copyEntry("support.loading").message}
          </InlineNotice>
        ) : snapshot.load === "failed" ? (
          <InlineNotice tone="danger" role="alert">
            <span>{copyEntry("support.load.failed").message}</span>{" "}
            <KitButton
              variant="secondary"
              size="sm"
              onClick={() => { void session.initialize(); }}
            >
              {copyEntry("action.retry").message}
            </KitButton>
          </InlineNotice>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col gap-4">
            {snapshot.entries.length === 0 ? (
              <StateEmpty
                title={copyEntry("support.empty").message}
                description={copyEntry("support.empty.body").message}
                action={<TopicSuggestions session={session} />}
              />
            ) : (
              <ul
                aria-label={copyEntry("support.history").message}
                className="flex max-h-[50dvh] flex-1 flex-col gap-3 overflow-y-auto"
              >
                {snapshot.entries.map((entry) => (
                  <EntryBubble key={entry.message_id} entry={entry} session={session} snapshot={snapshot} />
                ))}
                <li ref={endRef} aria-hidden="true" />
              </ul>
            )}
            {snapshot.connection === "offline" ? (
              <InlineNotice tone="warning" role="status">
                <span>{copyEntry("state.offline.banner").message}</span>{" "}
                <KitButton
                  variant="secondary"
                  size="sm"
                  onClick={() => {
                    void session.retryFailed();
                  }}
                >
                  {copyEntry("action.retry").message}
                </KitButton>
              </InlineNotice>
            ) : null}
            {composer}
          </div>
        )}
      </ScreenCard>
    </div>
  );
}

export function MobileSupportChat({ traceId }: Readonly<{ traceId?: string }>) {
  return <SupportChat platform="mobile" {...(traceId === undefined ? {} : { traceId })} />;
}

export function DesktopSupportChat({ traceId }: Readonly<{ traceId?: string }>) {
  return <SupportChat platform="desktop" {...(traceId === undefined ? {} : { traceId })} />;
}
