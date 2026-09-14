import { copyEntry } from "../../copy/runtime";
import {
  humanApi,
  type HumanApiClient,
  type SupportConversation,
  type SupportConversationState,
  type SupportMessage,
  type SupportShell,
  type SupportTopic,
  type TraceId,
} from "../api";
import { supportTopic } from "./topics";

export type SupportChatDelivery = "sending" | "sent" | "failed";
export type SupportChatConnection = "online" | "offline";
export type SupportChatLoad = "loading" | "ready" | "failed";
export type SupportFeedbackDelivery = "idle" | "sending" | "saved" | "failed";

export interface SupportChatEntry extends SupportMessage {
  readonly delivery: SupportChatDelivery;
}

export interface SupportChatSnapshot {
  readonly load: SupportChatLoad;
  readonly conversationId?: string;
  readonly conversationState?: SupportConversationState;
  readonly traceId?: TraceId;
  readonly draft: string;
  readonly topic?: SupportTopic;
  readonly connection: SupportChatConnection;
  readonly entries: readonly SupportChatEntry[];
  readonly feedback: ReadonlyMap<string, boolean>;
  readonly feedbackDelivery: ReadonlyMap<string, SupportFeedbackDelivery>;
  readonly awaitingFeedbackId?: string;
  readonly sending: boolean;
}

interface PendingMessage {
  readonly idempotencyKey: string;
  readonly messageId: string;
  readonly body: string;
  readonly topic: SupportTopic | undefined;
  state: "sending" | "failed";
}

export interface SupportChatSessionOptions {
  readonly shell: SupportShell;
  readonly traceId?: TraceId;
  readonly client?: HumanApiClient;
}

function requestIdentity(kind: "create" | "reply"): Readonly<{ key: string; messageId: string }> {
  const identity = crypto.randomUUID();
  return Object.freeze({ key: `support-${kind}-${identity}`, messageId: `pending_${identity}` });
}

function offlineFailure(error: unknown): boolean {
  return typeof navigator !== "undefined" && !navigator.onLine
    || error instanceof TypeError;
}

export class SupportChatSession {
  readonly #shell: SupportShell;
  readonly #traceId: TraceId | undefined;
  readonly #client: HumanApiClient;
  readonly #listeners = new Set<() => void>();
  #conversation: SupportConversation | undefined;
  #pending: PendingMessage[] = [];
  #draft = "";
  #topic: SupportTopic | undefined;
  #connection: SupportChatConnection = "online";
  #load: SupportChatLoad = "loading";
  #feedbackDelivery = new Map<string, SupportFeedbackDelivery>();
  #snapshot: SupportChatSnapshot;

  constructor(options: SupportChatSessionOptions) {
    this.#shell = options.shell;
    this.#traceId = options.traceId;
    this.#client = options.client ?? humanApi();
    this.#snapshot = this.#buildSnapshot();
  }

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => {
      this.#listeners.delete(listener);
    };
  };

  snapshot = (): SupportChatSnapshot => this.#snapshot;

  async initialize(): Promise<void> {
    this.#load = "loading";
    this.#notify();
    try {
      const page = await this.#client.supportList();
      this.#conversation = this.#selectConversation(page.conversations);
      this.#connection = "online";
      this.#load = "ready";
      this.#notify();
      await this.#markVisibleRepliesRead();
    } catch (error) {
      this.#connection = offlineFailure(error) ? "offline" : "online";
      this.#load = "failed";
      this.#notify();
    }
  }

  setOnline(online: boolean): void {
    this.#connection = online ? "online" : "offline";
    this.#notify();
  }

  seedTopic(id: SupportTopic): void {
    const topic = supportTopic(id);
    this.#topic = id;
    this.#draft = copyEntry(topic.seedKey).message;
    this.#notify();
  }

  setDraft(value: string): void {
    this.#draft = value.slice(0, 2_000);
    this.#notify();
  }

  async send(): Promise<void> {
    const body = this.#draft.trim();
    if (body.length === 0 || this.#pending.some((message) => message.state === "sending")) {
      return;
    }
    const identity = requestIdentity(this.#conversation === undefined ? "create" : "reply");
    const message: PendingMessage = {
      idempotencyKey: identity.key,
      messageId: identity.messageId,
      body,
      topic: this.#topic,
      state: "sending",
    };
    this.#pending.push(message);
    this.#draft = "";
    this.#topic = undefined;
    this.#notify();
    await this.#deliver(message);
  }

  async retry(messageId: string): Promise<void> {
    const message = this.#pending.find(
      (candidate) => candidate.messageId === messageId && candidate.state === "failed",
    );
    if (message === undefined) {
      return;
    }
    message.state = "sending";
    this.#notify();
    await this.#deliver(message);
  }

  async retryFailed(): Promise<void> {
    const failed = this.#pending.filter((candidate) => candidate.state === "failed");
    if (failed.length === 0) {
      await this.initialize();
      return;
    }
    for (const message of failed) {
      await this.retry(message.messageId);
    }
  }

  async sendFeedback(messageId: string, helpful: boolean): Promise<void> {
    if (this.#conversation === undefined || this.#feedbackDelivery.get(messageId) === "sending") {
      return;
    }
    this.#feedbackDelivery.set(messageId, "sending");
    this.#notify();
    try {
      this.#conversation = await this.#client.supportFeedback(
        this.#conversation.conversation_id,
        { message_id: messageId, helpful },
      );
      this.#feedbackDelivery.set(messageId, "saved");
      this.#connection = "online";
    } catch (error) {
      this.#feedbackDelivery.set(messageId, "failed");
      if (offlineFailure(error)) {
        this.#connection = "offline";
      }
    }
    this.#notify();
  }

  async #deliver(message: PendingMessage): Promise<void> {
    try {
      this.#conversation = this.#conversation === undefined
        ? await this.#client.supportCreate(
          {
            body: message.body,
            shell: this.#shell,
            ...(message.topic === undefined ? {} : { topic: message.topic }),
            ...(this.#traceId === undefined ? {} : { trace_id: this.#traceId }),
          },
          message.idempotencyKey,
        )
        : await this.#client.supportReply(
          this.#conversation.conversation_id,
          { body: message.body },
          message.idempotencyKey,
        );
      this.#pending = this.#pending.filter((candidate) => candidate.messageId !== message.messageId);
      this.#connection = "online";
      this.#load = "ready";
    } catch (error) {
      message.state = "failed";
      if (offlineFailure(error)) {
        this.#connection = "offline";
      }
    }
    this.#notify();
  }

  #selectConversation(conversations: readonly SupportConversation[]): SupportConversation | undefined {
    if (this.#traceId !== undefined) {
      return conversations.find((conversation) => conversation.trace_id === this.#traceId);
    }
    return conversations.find((conversation) => conversation.state !== "resolved");
  }

  async #markVisibleRepliesRead(): Promise<void> {
    const conversation = this.#conversation;
    const lastUnread = conversation?.messages.findLast(
      (message) => message.author === "support" && !message.read,
    );
    if (conversation === undefined || lastUnread === undefined) {
      return;
    }
    try {
      await this.#client.supportStatus(conversation.conversation_id);
      await this.#client.supportRead(
        conversation.conversation_id,
        { through_message_id: lastUnread.message_id },
      );
      this.#conversation = {
        ...conversation,
        messages: conversation.messages.map((message) => (
          message.author === "support" ? { ...message, read: true } : message
        )),
      };
      this.#notify();
    } catch (error) {
      if (offlineFailure(error)) {
        this.#connection = "offline";
        this.#notify();
      }
    }
  }

  #notify(): void {
    this.#snapshot = this.#buildSnapshot();
    for (const listener of this.#listeners) {
      listener();
    }
  }

  #buildSnapshot(): SupportChatSnapshot {
    const delivered: SupportChatEntry[] = (this.#conversation?.messages ?? []).map((message) => ({
      ...message,
      delivery: "sent" as const,
    }));
    const pending: SupportChatEntry[] = this.#pending.map((message) => ({
      message_id: message.messageId,
      author: "you" as const,
      body: message.body,
      sent_at: new Date().toISOString(),
      read: true,
      ...(message.topic === undefined ? {} : { topic: message.topic }),
      delivery: message.state,
    }));
    const feedback = new Map<string, boolean>(
      (this.#conversation?.feedback ?? []).map((entry) => [entry.message_id, entry.helpful]),
    );
    const awaiting = [...delivered].reverse().find((entry) => (
      entry.author === "support"
      && !feedback.has(entry.message_id)
      && this.#feedbackDelivery.get(entry.message_id) !== "saved"
    ));
    return Object.freeze({
      load: this.#load,
      ...(this.#conversation === undefined ? {} : {
        conversationId: this.#conversation.conversation_id,
        conversationState: this.#conversation.state,
      }),
      ...(this.#traceId === undefined ? {} : { traceId: this.#traceId }),
      draft: this.#draft,
      ...(this.#topic === undefined ? {} : { topic: this.#topic }),
      connection: this.#connection,
      entries: Object.freeze([...delivered, ...pending]),
      feedback,
      feedbackDelivery: new Map(this.#feedbackDelivery),
      ...(awaiting === undefined ? {} : { awaitingFeedbackId: awaiting.message_id }),
      sending: this.#pending.some((message) => message.state === "sending"),
    });
  }
}
