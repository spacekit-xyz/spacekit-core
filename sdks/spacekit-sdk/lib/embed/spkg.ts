import { unzipSync } from "fflate";
import { loadWebPackageFromLocal, type LoadWebPackageOptions } from "./packageLoader.js";
import type { AppPackageJSON, ContentRef, LoadedWebPackage } from "./types.js";

export const SPKG_MIMETYPE = "application/vnd.spacekit.spkg+zip";
export const SPKG_MAX_ENTRIES = 10_000;
export const SPKG_MAX_UNCOMPRESSED_BYTES = 512 * 1024 * 1024;

export type SpkgSource = Uint8Array | ArrayBuffer;

export interface OpenedSpkg {
  package: AppPackageJSON;
  files: Record<string, Uint8Array>;
}

function asBytes(source: SpkgSource): Uint8Array {
  return source instanceof Uint8Array ? source : new Uint8Array(source);
}

function validateFirstLocalEntry(archive: Uint8Array): void {
  const LOCAL_FILE_HEADER_SIZE = 30;
  const MIMETYPE_NAME = new TextEncoder().encode("mimetype");
  if (archive.byteLength < LOCAL_FILE_HEADER_SIZE) {
    throw new Error("SPKG is too short to contain a ZIP local file header");
  }

  const view = new DataView(archive.buffer, archive.byteOffset, archive.byteLength);
  if (view.getUint32(0, true) !== 0x04034b50) {
    throw new Error("SPKG must begin with a ZIP local file header");
  }
  if (view.getUint16(8, true) !== 0) {
    throw new Error("SPKG first entry must use ZIP method 0 (stored)");
  }

  const nameLength = view.getUint16(26, true);
  const extraLength = view.getUint16(28, true);
  const headerEnd = LOCAL_FILE_HEADER_SIZE + nameLength + extraLength;
  if (headerEnd > archive.byteLength) {
    throw new Error("SPKG first ZIP local file header is truncated");
  }
  if (nameLength !== MIMETYPE_NAME.byteLength) {
    throw new Error("SPKG first entry must be exactly mimetype");
  }
  for (let i = 0; i < MIMETYPE_NAME.byteLength; i++) {
    if (archive[LOCAL_FILE_HEADER_SIZE + i] !== MIMETYPE_NAME[i]) {
      throw new Error("SPKG first entry must be exactly mimetype");
    }
  }
}

function validateArchivePath(path: string): void {
  if (!path || path.includes("\0")) {
    throw new Error("SPKG contains an empty or invalid entry path");
  }
  if (path.startsWith("/") || /^[A-Za-z]:/.test(path)) {
    throw new Error(`SPKG contains an absolute entry path: ${path}`);
  }
  if (path.includes("\\")) {
    throw new Error(`SPKG entry paths must use forward slashes: ${path}`);
  }

  const parts = path.split("/");
  const finalEmptyDirectorySegment = path.endsWith("/");
  for (let i = 0; i < parts.length; i++) {
    const part = parts[i];
    if (part === "." || part === "..") {
      throw new Error(`SPKG contains a dot path segment: ${path}`);
    }
    if (part === "" && !(finalEmptyDirectorySegment && i === parts.length - 1)) {
      throw new Error(`SPKG contains an empty path segment: ${path}`);
    }
  }
}

function hashBytes(value: unknown, label: string): Uint8Array {
  if (typeof value === "string") {
    const hex = value.trim().toLowerCase();
    if (!/^[0-9a-f]{64}$/.test(hex)) {
      throw new Error(`${label} must be a 32-byte SHA-256 hash`);
    }
    return Uint8Array.from(hex.match(/../g)!, (byte) => Number.parseInt(byte, 16));
  }
  if (
    Array.isArray(value) &&
    value.length === 32 &&
    value.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255)
  ) {
    return Uint8Array.from(value);
  }
  throw new Error(`${label} must be a 32-byte SHA-256 hash`);
}

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

async function sha256(data: Uint8Array): Promise<Uint8Array> {
  if (typeof crypto === "undefined" || !crypto.subtle) {
    throw new Error("SHA-256 verification is unavailable in this environment");
  }
  const digest = await crypto.subtle.digest("SHA-256", Uint8Array.from(data));
  return new Uint8Array(digest);
}

function validatePackage(value: unknown): AppPackageJSON {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("SPKG manifest.json must contain an AppPackageJSON object");
  }
  const candidate = value as Partial<AppPackageJSON>;
  if (!candidate.manifest || typeof candidate.manifest !== "object") {
    throw new Error("SPKG manifest.json is missing manifest");
  }
  if (!Array.isArray(candidate.content_refs)) {
    throw new Error("SPKG manifest.json is missing content_refs");
  }
  if (typeof candidate.manifest.checksum !== "string" && !Array.isArray(candidate.manifest.checksum)) {
    throw new Error("SPKG manifest checksum is missing");
  }
  return candidate as AppPackageJSON;
}

function validateContentRef(ref: ContentRef, index: number): void {
  if (!ref || typeof ref !== "object") {
    throw new Error(`SPKG content_refs[${index}] is invalid`);
  }
  if (typeof ref.path !== "string" || !ref.path || ref.path.endsWith("/")) {
    throw new Error(`SPKG content_refs[${index}] has an invalid path`);
  }
  validateArchivePath(ref.path);
  if (!Number.isSafeInteger(ref.size) || ref.size < 0) {
    throw new Error(`SPKG content_refs[${index}] has an invalid size`);
  }
}

/**
 * Parse, extract, and fully verify an SPKG v1 archive.
 */
export async function openSpkg(source: SpkgSource): Promise<OpenedSpkg> {
  let entryCount = 0;
  let totalUncompressed = 0;
  const paths = new Set<string>();
  const archive = asBytes(source);
  validateFirstLocalEntry(archive);

  const entries = unzipSync(archive, {
    filter(entry) {
      entryCount++;
      if (entryCount > SPKG_MAX_ENTRIES) {
        throw new Error(`SPKG exceeds the ${SPKG_MAX_ENTRIES.toLocaleString()} entry limit`);
      }
      validateArchivePath(entry.name);
      if (paths.has(entry.name)) {
        throw new Error(`SPKG contains a duplicate entry path: ${entry.name}`);
      }
      paths.add(entry.name);

      if (!Number.isSafeInteger(entry.originalSize) || entry.originalSize < 0) {
        throw new Error(`SPKG entry has an invalid uncompressed size: ${entry.name}`);
      }
      totalUncompressed += entry.originalSize;
      if (totalUncompressed > SPKG_MAX_UNCOMPRESSED_BYTES) {
        throw new Error("SPKG exceeds the 512 MiB uncompressed size limit");
      }
      return true;
    },
  });

  const actualTotal = Object.values(entries).reduce((sum, bytes) => sum + bytes.byteLength, 0);
  if (actualTotal > SPKG_MAX_UNCOMPRESSED_BYTES) {
    throw new Error("SPKG exceeds the 512 MiB uncompressed size limit");
  }

  const mimetype = entries.mimetype;
  if (!mimetype || new TextDecoder().decode(mimetype) !== SPKG_MIMETYPE) {
    throw new Error(`SPKG mimetype must be exactly ${SPKG_MIMETYPE}`);
  }

  const manifestBytes = entries["manifest.json"];
  if (!manifestBytes) {
    throw new Error("SPKG is missing manifest.json");
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(manifestBytes));
  } catch (error) {
    throw new Error(`SPKG manifest.json is invalid: ${error instanceof Error ? error.message : String(error)}`);
  }
  const pkg = validatePackage(parsed);
  if (pkg.content_refs.length > SPKG_MAX_ENTRIES) {
    throw new Error(`SPKG exceeds the ${SPKG_MAX_ENTRIES.toLocaleString()} content ref limit`);
  }

  const refsByPath = new Map<string, ContentRef>();
  for (let i = 0; i < pkg.content_refs.length; i++) {
    const ref = pkg.content_refs[i];
    validateContentRef(ref, i);
    if (refsByPath.has(ref.path)) {
      throw new Error(`SPKG contains a duplicate content ref path: ${ref.path}`);
    }
    refsByPath.set(ref.path, ref);
  }

  for (const entryPath of Object.keys(entries)) {
    if (entryPath === "mimetype" || entryPath === "manifest.json") continue;
    if (entryPath.startsWith("signatures/")) continue;
    if (!entryPath.startsWith("payload/")) {
      throw new Error(`SPKG contains an unexpected top-level entry: ${entryPath}`);
    }
    if (entryPath.endsWith("/")) continue;
    const payloadPath = entryPath.slice("payload/".length);
    if (!refsByPath.has(payloadPath)) {
      throw new Error(`SPKG payload has no matching content ref: ${payloadPath}`);
    }
  }

  const files: Record<string, Uint8Array> = Object.create(null);
  const aggregateInput = new Uint8Array(pkg.content_refs.length * 32);
  for (let i = 0; i < pkg.content_refs.length; i++) {
    const ref = pkg.content_refs[i];
    const payload = entries[`payload/${ref.path}`];
    if (!payload) {
      throw new Error(`SPKG is missing payload/${ref.path}`);
    }
    if (payload.byteLength !== ref.size) {
      throw new Error(
        `SPKG payload size mismatch for ${ref.path}: expected ${ref.size}, got ${payload.byteLength}`,
      );
    }

    const expectedHash = hashBytes(ref.hash, `SPKG content_refs[${i}].hash`);
    const actualHash = await sha256(payload);
    if (hex(actualHash) !== hex(expectedHash)) {
      throw new Error(
        `SPKG payload hash mismatch for ${ref.path}: expected ${hex(expectedHash)}, got ${hex(actualHash)}`,
      );
    }
    aggregateInput.set(expectedHash, i * 32);
    files[ref.path] = payload;
  }

  const expectedChecksum = hashBytes(
    pkg.manifest.checksum,
    "SPKG manifest.checksum",
  );
  const actualChecksum = await sha256(aggregateInput);
  if (hex(actualChecksum) !== hex(expectedChecksum)) {
    throw new Error(
      `SPKG manifest checksum mismatch: expected ${hex(expectedChecksum)}, got ${hex(actualChecksum)}`,
    );
  }

  return { package: pkg, files };
}

export const parseSpkg = openSpkg;

export async function fetchSpkg(url: string | URL, init?: RequestInit): Promise<OpenedSpkg> {
  const response = await fetch(url, init);
  if (!response.ok) {
    throw new Error(`SPKG fetch failed (${response.status} ${response.statusText})`);
  }
  return openSpkg(await response.arrayBuffer());
}

function normalizeAppId(value: string | number[], label: string): string {
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase().replace(/^0x/, "");
    if (/^[0-9a-f]{64}$/.test(normalized)) return normalized;
  } else if (
    value.length === 32 &&
    value.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255)
  ) {
    return hex(Uint8Array.from(value));
  }
  throw new Error(`${label} must be a 32-byte hexadecimal app ID`);
}

export async function loadWebPackageFromSpkg(
  source: SpkgSource,
  options: LoadWebPackageOptions,
  expectedAppId?: string | number[],
): Promise<LoadedWebPackage> {
  const opened = await openSpkg(source);
  if (expectedAppId !== undefined) {
    const requested = normalizeAppId(expectedAppId, "Requested app ID");
    const archived = normalizeAppId(opened.package.app_id, "SPKG AppPackage app_id");
    if (archived !== requested) {
      throw new Error(`SPKG app_id mismatch: requested ${requested}, archive contains ${archived}`);
    }
  }
  return loadWebPackageFromLocal(opened.package, opened.files, options);
}
