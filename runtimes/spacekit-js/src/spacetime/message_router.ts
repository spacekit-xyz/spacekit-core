import { parseSpaceTimeCommand, type SpaceTimeAgentAction } from "./commands.js";
import type { SpaceTimeClient } from "./client.js";
import type { SpacekitMessageEnvelope } from "./message.js";

export interface SpacekitMessageRouterOptions {
  spacetimeClient: SpaceTimeClient;
  onChatMessage?: (message: SpacekitMessageEnvelope) => Promise<unknown> | unknown;
  onSystemMessage?: (message: SpacekitMessageEnvelope) => Promise<unknown> | unknown;
}

export type SpacekitMessageRouterResult =
  | { type: "spacetime_thread_created"; threadId: number }
  | { type: "spacetime_reply_created"; postId: number }
  | { type: "chat_routed"; result?: unknown }
  | { type: "system_routed"; result?: unknown }
  | { type: "error"; message: string }
  | { type: "unhandled_kind"; kind: string };

function isSpaceTimeAction(value: unknown): value is SpaceTimeAgentAction {
  if (!value || typeof value !== "object") {
    return false;
  }
  const action = value as { type?: unknown };
  return action.type === "create_thread" || action.type === "reply";
}

export async function routeSpacekitMessage(
  message: SpacekitMessageEnvelope,
  options: SpacekitMessageRouterOptions
): Promise<SpacekitMessageRouterResult> {
  if (!message.context?.did) {
    return { type: "error", message: "Missing DID in message context" };
  }

  switch (message.kind) {
    case "spacetime": {
      const action = isSpaceTimeAction(message.payload)
        ? message.payload
        : typeof message.payload === "string"
        ? parseSpaceTimeCommand(message.payload)
        : null;

      if (!action) {
        return { type: "error", message: "Invalid SpaceTime payload" };
      }

      switch (action.type) {
        case "create_thread": {
          const threadId = await options.spacetimeClient.createThread(
            action.title,
            action.text
          );
          return { type: "spacetime_thread_created", threadId };
        }
        case "reply": {
          const postId = await options.spacetimeClient.reply(
            action.threadId,
            action.parentPostId ?? null,
            action.text
          );
          return { type: "spacetime_reply_created", postId };
        }
      }
      return { type: "error", message: "Unhandled SpaceTime action" };
    }
    case "chat": {
      if (!options.onChatMessage) {
        return { type: "unhandled_kind", kind: "chat" };
      }
      const result = await options.onChatMessage(message);
      return { type: "chat_routed", result };
    }
    case "system": {
      if (!options.onSystemMessage) {
        return { type: "unhandled_kind", kind: "system" };
      }
      const result = await options.onSystemMessage(message);
      return { type: "system_routed", result };
    }
    default:
      return { type: "unhandled_kind", kind: message.kind };
  }
}
