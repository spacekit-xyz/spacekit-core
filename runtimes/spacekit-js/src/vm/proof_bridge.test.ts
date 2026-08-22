import test from "node:test";
import assert from "node:assert/strict";
import {
  substituteEnvInChainConfig,
  loadProofBridgeConfig,
  createAdaptersFromConfig,
  type ProofBridgeChainConfig,
  type ProofBridgeConfig,
  type ProofBridgeAdapter,
} from "./proof_bridge.js";

test("substituteEnvInChainConfig leaves hex key unchanged", () => {
  const config: ProofBridgeChainConfig = {
    enabled: true,
    chainId: "ethereum",
    privateKeyHex: "0xabcd1234",
  };
  const out = substituteEnvInChainConfig(config);
  assert.equal(out.privateKeyHex, "0xabcd1234");
});

test("substituteEnvInChainConfig substitutes env var when set", () => {
  const envKey = "SPACEKIT_TEST_PROOF_BRIDGE_KEY";
  const prev = process.env[envKey];
  process.env[envKey] = "secret-from-env";
  try {
    const config: ProofBridgeChainConfig = {
      enabled: true,
      chainId: "ethereum",
      privateKeyHex: envKey,
    };
    const out = substituteEnvInChainConfig(config);
    assert.equal(out.privateKeyHex, "secret-from-env");
  } finally {
    if (prev !== undefined) process.env[envKey] = prev;
    else delete process.env[envKey];
  }
});

test("substituteEnvInChainConfig leaves env var name when unset", () => {
  const config: ProofBridgeChainConfig = {
    enabled: true,
    chainId: "ethereum",
    privateKeyHex: "SPACEKIT_NONEXISTENT_VAR_12345",
  };
  const out = substituteEnvInChainConfig(config);
  assert.equal(out.privateKeyHex, "SPACEKIT_NONEXISTENT_VAR_12345");
});

test("loadProofBridgeConfig inline returns enabled chains only", async () => {
  const config: ProofBridgeConfig = {
    source: "inline",
    chains: {
      eth: {
        enabled: true,
        chainId: "ethereum",
        rpcUrl: "https://eth.llamarpc.com",
      },
      btc: {
        enabled: false,
        chainId: "bitcoin",
      },
      sol: {
        enabled: true,
        chainId: "solana",
        rpcUrl: "https://api.mainnet-beta.solana.com",
      },
    },
  };
  const list = await loadProofBridgeConfig(config);
  assert.equal(list.length, 2);
  const ids = list.map((c) => c.chainId).sort();
  assert.deepEqual(ids, ["ethereum", "solana"]);
});

test("loadProofBridgeConfig inline returns empty when no chains", async () => {
  const list = await loadProofBridgeConfig({ source: "inline" });
  assert.equal(list.length, 0);
});

test("loadProofBridgeConfig url fetches and parses chains", async () => {
  const origFetch = globalThis.fetch;
  globalThis.fetch = async (url: URL | RequestInfo) => {
    assert.equal(url, "https://example.com/bridge-config");
    return {
      ok: true,
      json: async () => ({
        chains: {
          ethereum: {
            enabled: true,
            chainId: "ethereum:mainnet",
            rpcUrl: "https://eth.llamarpc.com",
            contractAddress: "0x1234",
          },
        },
      }),
    } as Response;
  };
  try {
    const list = await loadProofBridgeConfig({
      source: "url",
      configUrl: "https://example.com/bridge-config",
    });
    assert.equal(list.length, 1);
    assert.equal(list[0].chainId, "ethereum:mainnet");
    assert.equal(list[0].contractAddress, "0x1234");
  } finally {
    globalThis.fetch = origFetch;
  }
});

test("loadProofBridgeConfig url accepts bridgeConfig wrapper", async () => {
  const origFetch = globalThis.fetch;
  globalThis.fetch = async () =>
    ({
      ok: true,
      json: async () => ({
        bridgeConfig: {
          chains: {
            bitcoin: { enabled: true, chainId: "bitcoin", indexerUrl: "https://indexer.example.com" },
          },
        },
      }),
    }) as Response;
  try {
    const list = await loadProofBridgeConfig({
      source: "url",
      configUrl: "https://example.com/c",
    });
    assert.equal(list.length, 1);
    assert.equal(list[0].chainId, "bitcoin");
  } finally {
    globalThis.fetch = origFetch;
  }
});

test("loadProofBridgeConfig url throws on fetch failure", async () => {
  const origFetch = globalThis.fetch;
  globalThis.fetch = async () => ({ ok: false, status: 404, statusText: "Not Found" }) as Response;
  try {
    await assert.rejects(
      async () =>
        loadProofBridgeConfig({
          source: "url",
          configUrl: "https://example.com/missing",
        }),
      /Proof bridge config fetch failed: 404/
    );
  } finally {
    globalThis.fetch = origFetch;
  }
});

test("createAdaptersFromConfig uses custom registry", async () => {
  const fakeAdapter: ProofBridgeAdapter = {
    chainId: "test-chain",
    isReady: () => true,
    submit: async () => ({ success: true, id: "tx-1" }),
  };
  const registry = {
    create: async (config: ProofBridgeChainConfig): Promise<ProofBridgeAdapter | null> =>
      config.chainId === "custom" ? fakeAdapter : null,
  };
  const adapters = await createAdaptersFromConfig(
    [
      { enabled: true, chainId: "custom" },
      { enabled: true, chainId: "other" },
    ],
    { adapterRegistry: registry }
  );
  assert.equal(adapters.length, 1);
  assert.equal(adapters[0].chainId, "test-chain");
  const result = await adapters[0].submit({ kind: "state_root", blockHeight: 1, stateRoot: "0xab" });
  assert.equal(result.success, true);
  assert.equal(result.id, "tx-1");
});

test("createAdaptersFromConfig with registry returning null yields empty list", async () => {
  const adapters = await createAdaptersFromConfig(
    [{ enabled: true, chainId: "ethereum" }],
    { adapterRegistry: { create: async () => null } }
  );
  assert.equal(adapters.length, 0);
});
