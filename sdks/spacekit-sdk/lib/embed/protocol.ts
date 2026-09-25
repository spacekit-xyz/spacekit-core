/**
 * SpaceKit embed protocol, version 1.
 *
 * The wire contract between a host page (which mounts a SpaceKit web package)
 * and the sandboxed frame the package runs in. The full specification lives in
 * `sdks/spacekit-apps/SPACEKIT-EMBED-PROTOCOL.md`; keep the two in sync.
 *
 * Handshake (window messages, the only ones exchanged over `postMessage`):
 *   frame → host   { type: "spacekit-frame-ready", v }
 *   host  → frame  { type: "spacekit-frame-load",  v, appId, html, files, localSeed }
 *                  + one transferred MessagePort
 *   frame → host   { type: "spacekit-frame-error", v, message }   (load refused)
 *   frame → host   { t: "loaded" } on the port                     (load accepted)
 *
 * Everything after the handshake travels over that MessagePort, never over
 * `window.postMessage`, so no reply is ever broadcast with a `"*"` target:
 *   app  → host    { t: "call",  id, module, method, params }
 *   host → app     { t: "res",   id, result } | { t: "res", id, error }
 *   host → app     { t: "event", topic, msg }
 */

export const SPACEKIT_EMBED_PROTOCOL_VERSION = 1;

export const FRAME_READY = "spacekit-frame-ready";
export const FRAME_LOAD = "spacekit-frame-load";
export const FRAME_ERROR = "spacekit-frame-error";

/** One verified package file handed to the frame, keyed by its placeholder. */
export interface FrameLoadFile {
  /** Unique token that appears in `html` wherever this file's URL belongs. */
  ph: string;
  mime: string;
  bytes: ArrayBuffer;
}

export interface FrameReadyMessage {
  type: typeof FRAME_READY;
  v: number;
}

export interface FrameErrorMessage {
  type: typeof FRAME_ERROR;
  v: number;
  message: string;
}

export interface FrameLoadMessage {
  type: typeof FRAME_LOAD;
  v: number;
  appId: string;
  html: string;
  files: FrameLoadFile[];
  /**
   * Snapshot of the app's persisted `localStorage` shim, used only when the
   * frame's own origin has no storage (opaque isolation).
   */
  localSeed: Record<string, string> | null;
}

export interface PortCallMessage {
  t: "call";
  id: number;
  module: string;
  method: string;
  params: Record<string, unknown>;
}

export type PortHostMessage =
  | { t: "res"; id: number; result?: unknown; error?: string }
  | { t: "event"; topic: string; msg: unknown };

/** Storage-module key prefix used by the guest `localStorage` shim. */
export const LOCAL_STORAGE_SHIM_PREFIX = "__ls:";

export function isFrameReadyMessage(data: unknown): data is FrameReadyMessage {
  return (
    !!data &&
    typeof data === "object" &&
    (data as { type?: unknown }).type === FRAME_READY &&
    (data as { v?: unknown }).v === SPACEKIT_EMBED_PROTOCOL_VERSION
  );
}

export function isFrameErrorMessage(data: unknown): data is FrameErrorMessage {
  return (
    !!data &&
    typeof data === "object" &&
    (data as { type?: unknown }).type === FRAME_ERROR &&
    typeof (data as { message?: unknown }).message === "string"
  );
}

export function isPortCallMessage(data: unknown): data is PortCallMessage {
  if (!data || typeof data !== "object") return false;
  const d = data as Partial<PortCallMessage>;
  return (
    d.t === "call" &&
    typeof d.id === "number" &&
    typeof d.module === "string" &&
    typeof d.method === "string"
  );
}
