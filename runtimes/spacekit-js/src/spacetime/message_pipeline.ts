import { createSpacekitMessage, type SpacekitMessageContext } from "./message.js";
import type { SpacekitMessageEnvelope } from "./message.js";
import type { SpacekitMessagingAdapter } from "./messaging_adapter.js";
import { routeSpaceTimeMessage } from "./router.js";
import type { SpaceTimeClient } from "./client.js";

export interface MessagePipelineOptions {
  classifier: (input: string) => Promise<{ intent: string; confidence: number }>;
  spacetimeClient: SpaceTimeClient;
  messagingAdapter?: SpacekitMessagingAdapter;
}

export async function ingestSpacekitTextMessage(
  rawText: string,
  context: SpacekitMessageContext,
  options: MessagePipelineOptions
): Promise<unknown> {
  const { intent } = await options.classifier(rawText);

  if (intent === "spacekit_message") {
    return routeSpaceTimeMessage(rawText, options.classifier, options.spacetimeClient, context.did);
  }

  if (options.messagingAdapter) {
    const envelope = createSpacekitMessage("chat", rawText, context);
    await options.messagingAdapter.send(envelope);
    return { type: "chat_routed" };
  }

  return { type: "unhandled_intent", intent };
}

export async function ingestSpacekitEnvelope(
  envelope: SpacekitMessageEnvelope,
  options: MessagePipelineOptions
): Promise<unknown> {
  if (typeof envelope.payload === "string") {
    return ingestSpacekitTextMessage(envelope.payload, envelope.context, options);
  }
  if (options.messagingAdapter) {
    await options.messagingAdapter.send(envelope);
    return { type: "message_forwarded" };
  }
  return { type: "unhandled_payload" };
}
