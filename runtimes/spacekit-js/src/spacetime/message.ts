export type SpacekitMessageKind =
  | "spacetime"
  | "chat"
  | "system";

export interface SpacekitMessageContext {
  did: string;
  timestamp: number;
  source?: string;
}

export interface SpacekitMessageEnvelope<TPayload = unknown> {
  kind: SpacekitMessageKind;
  payload: TPayload;
  context: SpacekitMessageContext;
}

export function createSpacekitMessage<TPayload>(
  kind: SpacekitMessageKind,
  payload: TPayload,
  context: SpacekitMessageContext
): SpacekitMessageEnvelope<TPayload> {
  return { kind, payload, context };
}
