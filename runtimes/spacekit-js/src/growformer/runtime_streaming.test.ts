import test from "node:test";
import assert from "node:assert/strict";
import {
  fetchGrowformerBrainBytes,
  initGrowformerHostWithBrainFromUrl,
  type GrowformerBrainFetchProgress,
  type GrowformerBrainFetcher,
} from "./runtime.js";

const encoder = new TextEncoder();

function chunkedResponse(chunks: Uint8Array[], contentLength?: string): Response {
  return new Response(
    new ReadableStream<Uint8Array>({
      pull(controller) {
        const chunk = chunks.shift();
        if (chunk) {
          controller.enqueue(chunk);
        } else {
          controller.close();
        }
      },
    }),
    {
      status: 200,
      headers: contentLength === undefined ? undefined : { "content-length": contentLength },
    },
  );
}

test("streams a known-length brain into its declared allocation", async () => {
  const progress: GrowformerBrainFetchProgress[] = [];
  const bytes = await fetchGrowformerBrainBytes("https://example.test/brain.bin", {
    fetcher: async () =>
      chunkedResponse([Uint8Array.of(1, 2), Uint8Array.of(3, 4, 5)], "5"),
    onProgress: (entry) => progress.push(entry),
  });

  assert.deepEqual(bytes, Uint8Array.of(1, 2, 3, 4, 5));
  assert.deepEqual(progress, [
    { phase: "download", bytesReceived: 0, totalBytes: 5 },
    { phase: "download", bytesReceived: 2, totalBytes: 5 },
    { phase: "download", bytesReceived: 5, totalBytes: 5 },
  ]);
});

test("streams unknown-length chunks and concatenates them", async () => {
  const progress: GrowformerBrainFetchProgress[] = [];
  const bytes = await fetchGrowformerBrainBytes("https://example.test/brain.bin", {
    fetcher: async () =>
      chunkedResponse([Uint8Array.of(9), Uint8Array.of(8, 7), Uint8Array.of(6)]),
    onProgress: (entry) => progress.push(entry),
  });

  assert.deepEqual(bytes, Uint8Array.of(9, 8, 7, 6));
  assert.deepEqual(progress.map(({ bytesReceived }) => bytesReceived), [0, 1, 3, 4]);
  assert.ok(progress.every(({ totalBytes }) => totalBytes === undefined));
});

test("propagates the AbortSignal and stops an incremental read", async () => {
  const controller = new AbortController();
  let receivedSignal: AbortSignal | null | undefined;
  let pulls = 0;
  const fetcher: GrowformerBrainFetcher = async (_input, init) => {
    receivedSignal = init?.signal;
    return new Response(
      new ReadableStream<Uint8Array>({
        pull(streamController) {
          pulls += 1;
          streamController.enqueue(Uint8Array.of(pulls));
          if (pulls === 1) {
            controller.abort();
          }
        },
      }),
    );
  };

  await assert.rejects(
    fetchGrowformerBrainBytes("https://example.test/brain.bin", {
      fetcher,
      signal: controller.signal,
    }),
    (error: unknown) => error instanceof DOMException && error.name === "AbortError",
  );
  assert.equal(receivedSignal, controller.signal);
});

test("rejects over-limit declarations before reading the body", async () => {
  let arrayBufferCalled = false;
  const response = {
    ok: true,
    status: 200,
    headers: new Headers({ "content-length": "6" }),
    body: null,
    async arrayBuffer() {
      arrayBufferCalled = true;
      return new Uint8Array(6).buffer;
    },
  } as Response;

  await assert.rejects(
    fetchGrowformerBrainBytes("https://example.test/brain.bin", {
      fetcher: async () => response,
      maxBytes: 5,
    }),
    /exceeds maxBytes/,
  );
  assert.equal(arrayBufferCalled, false);
});

test("rejects an unknown-length body as soon as it exceeds maxBytes", async () => {
  await assert.rejects(
    fetchGrowformerBrainBytes("https://example.test/brain.bin", {
      fetcher: async () =>
        chunkedResponse([Uint8Array.of(1, 2, 3), Uint8Array.of(4, 5, 6)]),
      maxBytes: 5,
    }),
    /exceeds maxBytes/,
  );
});

test("rejects malformed, negative, and unsafe Content-Length values", async (t) => {
  for (const value of ["5x", "-1", "9007199254740992"]) {
    await t.test(value, async () => {
      await assert.rejects(
        fetchGrowformerBrainBytes("https://example.test/brain.bin", {
          fetcher: async () => chunkedResponse([], value),
        }),
        /Invalid Growformer brain Content-Length/,
      );
    });
  }
});

test("accepts a case-insensitive matching SHA-256 and rejects mismatches", async () => {
  const hello = encoder.encode("hello");
  const digest = "2CF24DBA5FB0A30E26E83B2AC5B9E29E1B161E5C1FA7425E73043362938B9824";
  const phases: string[] = [];
  const bytes = await fetchGrowformerBrainBytes("https://example.test/brain.bin", {
    fetcher: async () => chunkedResponse([hello], "5"),
    expectedSha256Hex: digest,
    onProgress: ({ phase }) => phases.push(phase),
  });
  assert.deepEqual(bytes, hello);
  assert.deepEqual(phases, ["download", "download", "verify"]);

  await assert.rejects(
    fetchGrowformerBrainBytes("https://example.test/brain.bin", {
      fetcher: async () => chunkedResponse([hello], "5"),
      expectedSha256Hex: "0".repeat(64),
    }),
    /SHA-256 mismatch/,
  );
});

test("reports HTTP errors", async () => {
  await assert.rejects(
    fetchGrowformerBrainBytes("https://example.test/missing.bin", {
      fetcher: async () => new Response(null, { status: 404 }),
    }),
    /fetch failed: https:\/\/example\.test\/missing\.bin \(404\)/,
  );
});

test("falls back to arrayBuffer when Response.body is unavailable", async () => {
  let arrayBufferCalled = false;
  const raw = Uint8Array.of(4, 3, 2, 1);
  const response = {
    ok: true,
    status: 200,
    headers: new Headers({ "content-length": "4" }),
    body: null,
    async arrayBuffer() {
      arrayBufferCalled = true;
      return raw.buffer.slice(0);
    },
  } as Response;

  const bytes = await fetchGrowformerBrainBytes("https://example.test/brain.bin", {
    fetcher: async () => response,
  });
  assert.equal(arrayBufferCalled, true);
  assert.deepEqual(bytes, raw);

  await assert.rejects(
    fetchGrowformerBrainBytes("https://example.test/brain.bin", {
      fetcher: async () => ({
        ...response,
        headers: new Headers(),
        arrayBuffer: async () => new Uint8Array(6).buffer,
      }) as Response,
      maxBytes: 5,
    }),
    /exceeds maxBytes/,
  );
});

test("reports download, verify, and load phases in order", async () => {
  const phases: string[] = [];
  const script = [
    "export default async function () {}",
    "export function growformer_init() {}",
    "export function growformer_load_brain(data) { globalThis.__spacekitLoadedBrain = Array.from(data); }",
    "export function growformer_ready() { return true; }",
  ].join("\n");

  await initGrowformerHostWithBrainFromUrl("https://example.test/brain.bin", {
    fetcher: async () => chunkedResponse([encoder.encode("hello")], "5"),
    expectedSha256Hex: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
    scriptModuleUrl: `data:text/javascript,${encodeURIComponent(script)}`,
    onProgress: ({ phase }) => phases.push(phase),
  });

  assert.deepEqual(phases, ["download", "download", "verify", "load"]);
  assert.deepEqual(
    (globalThis as typeof globalThis & { __spacekitLoadedBrain?: number[] }).__spacekitLoadedBrain,
    Array.from(encoder.encode("hello")),
  );
});
