/**
 * §18.2 passive RealTraffic capture for browser inference (GROUNDING_LOOP_SPEC §18).
 *
 * Every browser inference funnels through the `runtime.ts` host functions
 * (`growformerHost{Converse,Generation,Codegen}Json`), which the VM `spacekit_agent` host imports
 * call when an agent contract runs. This module records the real user prompt — the scarce resource
 * for deployment certification — tagged `RealTraffic`, buffers it durably in `localStorage`, and
 * optionally POSTs batches to a collection endpoint that appends to the same `traffic_<agent>.jsonl`
 * schema the offline certifier reads (growformer `--audit-capture`).
 *
 * Discipline (mirrors the CLI capture; §18.3 blind-label rule):
 * - Label-free. It records what the user said and, as a *triage-only* signal, the incumbent reply.
 *   It never assigns or reads a routing label — only blind human adjudication may label, offline.
 * - Pure side-effect, best-effort. Capture failure must never affect inference.
 * - Disabled by default. The host app opts in via `configureGrowformerCapture`, so collecting user
 *   prompts is an explicit product decision with its own consent/ToS coverage — never silent.
 */

export interface GrowformerTrafficCapture {
  phrase: string;
  agent: string;
  response: string | null;
  timestamp_unix: number;
  session_id: string;
  provenance: { kind: "RealTraffic"; phrase_id: string; derived_from: string[] };
}

interface CaptureConfig {
  enabled: boolean;
  endpoint: string | null;
  agent: string;
  sessionId: string;
  maxBuffer: number;
  flushThreshold: number;
}

const STORAGE_KEY = "growformer_capture_buffer_v1";

const config: CaptureConfig = {
  enabled: false,
  endpoint: null,
  agent: "unknown",
  sessionId: defaultSessionId(),
  maxBuffer: 5000,
  flushThreshold: 16,
};

function defaultSessionId(): string {
  return `web_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

function nowUnix(): number {
  return Math.floor(Date.now() / 1000);
}

/**
 * Opt in / configure capture. The host app (e.g. Agent Hub) calls this. `agent` is the only field
 * that must track the active agent so records land in the right `traffic_<agent>.jsonl`. A new
 * `sessionId` (per conversation) preserves the §18.3 rephrase signal; omit to keep the current one.
 */
export function configureGrowformerCapture(
  partial: Partial<Omit<CaptureConfig, "sessionId">> & { sessionId?: string },
): void {
  if (typeof partial.enabled === "boolean") config.enabled = partial.enabled;
  if (partial.endpoint !== undefined) config.endpoint = partial.endpoint || null;
  if (typeof partial.agent === "string" && partial.agent.trim()) config.agent = partial.agent.trim();
  if (typeof partial.sessionId === "string" && partial.sessionId.trim()) {
    config.sessionId = partial.sessionId.trim();
  }
  if (typeof partial.maxBuffer === "number" && partial.maxBuffer > 0) config.maxBuffer = partial.maxBuffer;
  if (typeof partial.flushThreshold === "number" && partial.flushThreshold > 0) {
    config.flushThreshold = partial.flushThreshold;
  }
}

/** Start a fresh capture session (call when a new conversation begins). */
export function newGrowformerCaptureSession(): string {
  config.sessionId = defaultSessionId();
  return config.sessionId;
}

export function isGrowformerCaptureEnabled(): boolean {
  return config.enabled;
}

/**
 * Pluggable transport. When set, `flushGrowformerCapture` hands the buffered batch to this
 * function instead of POSTing NDJSON to `endpoint`. Return `true` on durable acceptance (the
 * buffer is then cleared). Lets the host app route captures to whatever backend it already has
 * (e.g. a spacekit storage-node `PUT /api/documents/...`), keeping this module transport-agnostic.
 */
export type GrowformerCaptureUploader = (
  records: GrowformerTrafficCapture[],
) => Promise<boolean>;

let uploader: GrowformerCaptureUploader | null = null;

export function setGrowformerCaptureUploader(fn: GrowformerCaptureUploader | null): void {
  uploader = fn;
}

function loadBuffer(): GrowformerTrafficCapture[] {
  try {
    const raw = globalThis.localStorage?.getItem(STORAGE_KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw);
    return Array.isArray(arr) ? (arr as GrowformerTrafficCapture[]) : [];
  } catch {
    return [];
  }
}

function saveBuffer(buf: GrowformerTrafficCapture[]): void {
  try {
    globalThis.localStorage?.setItem(STORAGE_KEY, JSON.stringify(buf));
  } catch {
    /* private mode / quota — capture is best-effort */
  }
}

/** Best-effort extraction of the reply text from the host JSON (triage signal only, never a label). */
function extractResponseText(responseJson: string | null): string | null {
  if (!responseJson) return null;
  try {
    const v = JSON.parse(responseJson) as { text?: unknown };
    if (v && typeof v === "object" && typeof v.text === "string") return v.text;
  } catch {
    /* ignore */
  }
  return null;
}

/** Record one real inference prompt. Called from the runtime host functions. Never throws. */
export function recordGrowformerTraffic(prompt: string, responseJson: string | null): void {
  if (!config.enabled) return;
  const phrase = (prompt ?? "").trim();
  if (!phrase) return;
  try {
    const rec: GrowformerTrafficCapture = {
      phrase,
      agent: config.agent,
      response: extractResponseText(responseJson),
      timestamp_unix: nowUnix(),
      session_id: config.sessionId,
      provenance: { kind: "RealTraffic", phrase_id: phrase, derived_from: [] },
    };
    const buf = loadBuffer();
    buf.push(rec);
    if (buf.length > config.maxBuffer) buf.splice(0, buf.length - config.maxBuffer);
    saveBuffer(buf);
    if ((uploader || config.endpoint) && buf.length >= config.flushThreshold) {
      void flushGrowformerCapture();
    }
  } catch {
    /* best-effort */
  }
}

let flushing = false;

/**
 * POST the buffered records to the configured endpoint as NDJSON; clears the buffer on success.
 * The collector appends the lines verbatim to `traffic_<agent>.jsonl`. No-op without an endpoint
 * (records stay durably buffered and can be drained via `exportGrowformerCaptureJsonl`).
 */
export async function flushGrowformerCapture(): Promise<number> {
  if (flushing || (!uploader && !config.endpoint)) return 0;
  const buf = loadBuffer();
  if (buf.length === 0) return 0;
  flushing = true;
  try {
    let ok = false;
    if (uploader) {
      ok = await uploader(buf);
    } else if (config.endpoint) {
      const body = buf.map((r) => JSON.stringify(r)).join("\n");
      const res = await fetch(config.endpoint, {
        method: "POST",
        headers: { "content-type": "application/x-ndjson" },
        body,
        keepalive: true,
      });
      ok = res.ok;
    }
    if (ok) {
      saveBuffer([]);
      return buf.length;
    }
    return 0;
  } catch {
    return 0; // keep buffer for next flush
  } finally {
    flushing = false;
  }
}

/** Export the durable buffer as JSONL (the exact `traffic_<agent>.jsonl` schema) for manual collection. */
export function exportGrowformerCaptureJsonl(): string {
  return loadBuffer()
    .map((r) => JSON.stringify(r))
    .join("\n");
}

export function growformerCaptureBufferSize(): number {
  return loadBuffer().length;
}

export function clearGrowformerCaptureBuffer(): void {
  saveBuffer([]);
}
