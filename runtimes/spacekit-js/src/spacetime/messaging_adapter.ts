import type { SpacekitMessageEnvelope } from "./message.js";

export interface SpacekitMessagingAdapter {
  send(message: SpacekitMessageEnvelope): Promise<void>;
}

export interface HttpMessagingAdapterOptions {
  baseUrl: string;
  endpointPath?: string;
  headers?: Record<string, string>;
}

export type MessageListener = (message: SpacekitMessageEnvelope) => void;

export class NoopMessagingAdapter implements SpacekitMessagingAdapter {
  async send(_message: SpacekitMessageEnvelope): Promise<void> {
    // Intentionally no-op for local/dev environments.
  }
}

export class LocalMessagingAdapter implements SpacekitMessagingAdapter {
  private listeners = new Set<MessageListener>();

  async send(message: SpacekitMessageEnvelope): Promise<void> {
    for (const listener of this.listeners) {
      listener(message);
    }
  }

  subscribe(listener: MessageListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }
}

export class HttpMessagingAdapter implements SpacekitMessagingAdapter {
  private baseUrl: string;
  private endpointPath: string;
  private headers: Record<string, string>;
  private conversationType: "direct" | "group";
  private recipientDid?: string;
  private groupId?: string;

  constructor(options: HttpMessagingAdapterOptions) {
    this.baseUrl = options.baseUrl.replace(/\/$/, "");
    this.endpointPath = options.endpointPath ?? "/api/messages/envelope";
    this.headers = options.headers ?? {};
    this.conversationType = "direct";
  }

  async send(message: SpacekitMessageEnvelope): Promise<void> {
    const url = `${this.baseUrl}${this.endpointPath}`;
    const res = await fetch(url, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...this.headers,
      },
      body: JSON.stringify({
        message,
        conversation_type: this.conversationType,
        recipient_did: this.recipientDid,
        group_id: this.groupId,
      }),
    });

    if (!res.ok) {
      throw new Error(`Messaging adapter failed: ${res.status}`);
    }
  }

  setDirectRecipient(did: string) {
    this.conversationType = "direct";
    this.recipientDid = did;
    this.groupId = undefined;
  }

  setGroupTarget(groupId: string) {
    this.conversationType = "group";
    this.groupId = groupId;
    this.recipientDid = undefined;
  }
}
