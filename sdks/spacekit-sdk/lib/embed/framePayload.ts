/**
 * Turns a verified web package into the `spacekit-frame-load` payload.
 *
 * The host never creates URLs for the app's assets. It rewrites the entry HTML
 * to reference unguessable placeholders and hands the verified bytes to the
 * frame, whose bootstrap mints `blob:` URLs in the frame's own origin and
 * substitutes them (see frameBootstrap.ts).
 */

import { injectSdkBridgeIntoHtml } from "./injectShim.js";
import type { VerifiedWebPackageFiles } from "./packageLoader.js";
import type { FrameLoadFile } from "./protocol.js";
import type { AppManifest, EmbedEndpoints } from "./types.js";

export interface FramePayloadOptions {
  parentOrigin: string;
  endpoints: EmbedEndpoints;
  identityDid: string | null;
  contentFit?: "fill" | "contain";
}

export interface FramePayload {
  html: string;
  files: FrameLoadFile[];
  entryPath: string;
}

export function findHtmlEntryPath(manifest: AppManifest): string | null {
  const eps = (manifest.entry_points ?? []) as Array<Record<string, unknown>>;
  const main = eps.find((ep) => "Html" in ep && (ep.Html as { is_main?: boolean })?.is_main);
  const entry = main ?? eps.find((ep) => "Html" in ep);
  if (entry && "Html" in entry) {
    const path = (entry.Html as { path?: unknown }).path;
    return typeof path === "string" ? path : null;
  }
  return null;
}

function randomNonce(): string {
  const bytes = new Uint8Array(12);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function uint8ToBase64(bytes: Uint8Array): string {
  const chunkSize = 0x8000;
  const parts: string[] = [];
  for (let i = 0; i < bytes.length; i += chunkSize) {
    parts.push(String.fromCharCode(...bytes.subarray(i, i + chunkSize)));
  }
  return btoa(parts.join(""));
}

function copyToArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  const out = new ArrayBuffer(bytes.byteLength);
  new Uint8Array(out).set(bytes);
  return out;
}

export function buildFramePayload(
  verified: VerifiedWebPackageFiles,
  options: FramePayloadOptions,
): FramePayload {
  const entryPath = findHtmlEntryPath(verified.pkg.manifest);
  if (!entryPath) throw new Error("App package has no HTML entry point");
  const entry = verified.files.get(entryPath);
  if (!entry) throw new Error(`App package is missing its HTML entry (${entryPath})`);

  const nonce = randomNonce();
  const assetUrls = new Map<string, string>();
  const files: FrameLoadFile[] = [];
  let kyberWasmBase64: string | undefined;
  let i = 0;
  for (const [path, file] of verified.files) {
    const ph = `sk-asset-${nonce}-${i++}`;
    assetUrls.set(path, ph);
    files.push({ ph, mime: file.mime, bytes: copyToArrayBuffer(file.bytes) });
    if (!kyberWasmBase64 && path.endsWith(".wasm")) kyberWasmBase64 = uint8ToBase64(file.bytes);
  }

  const html = injectSdkBridgeIntoHtml(new TextDecoder().decode(entry.bytes), assetUrls, {
    appId: verified.appId,
    parentOrigin: options.parentOrigin,
    endpoints: options.endpoints,
    identityDid: options.identityDid,
    kyberWasmBase64,
    contentFit: options.contentFit,
  });

  return { html, files, entryPath };
}
