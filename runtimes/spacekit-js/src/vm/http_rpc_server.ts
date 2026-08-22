import http from "node:http";
import { createJsonRpcHandler } from "./json_rpc.js";
import type { SpacekitVm } from "./spacekitvm.js";

export interface RpcServerOptions {
  port?: number;
  host?: string;
  rateLimit?: {
    windowMs: number;
    maxRequests: number;
  };
  allowlist?: string[];
  apiKey?: string;
}

export function startJsonRpcServer(vm: SpacekitVm, options: RpcServerOptions = {}) {
  const port = options.port ?? 8747;
  const host = options.host ?? "127.0.0.1";
  const handler = createJsonRpcHandler(vm);
  const rateLimit = options.rateLimit ?? { windowMs: 60_000, maxRequests: 120 };
  const allowlist = options.allowlist ?? [];
  const apiKey = options.apiKey;
  const rateTable = new Map<string, { count: number; resetAt: number }>();
  const corsHeaders = {
    "Access-Control-Allow-Origin": "*",
    "Access-Control-Allow-Methods": "POST, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type, Authorization",
  };

  const server = http.createServer(async (req, res) => {
    if (req.method === "OPTIONS") {
      res.writeHead(204, corsHeaders);
      res.end();
      return;
    }

    if (!req.url || req.method !== "POST") {
      res.writeHead(404, { "Content-Type": "application/json", ...corsHeaders });
      res.end(JSON.stringify({ error: "Not found" }));
      return;
    }

    const clientIp = req.socket.remoteAddress ?? "unknown";
    let body = "";
    req.on("data", (chunk) => {
      body += chunk;
    });
    req.on("end", async () => {
      try {
        if (allowlist.length > 0 && !allowlist.includes(clientIp)) {
          res.writeHead(403, { "Content-Type": "application/json", ...corsHeaders });
          res.end(JSON.stringify({ error: "IP not allowed" }));
          return;
        }
        if (apiKey) {
          const auth = req.headers["authorization"];
          const keyHeader = req.headers["x-api-key"];
          const token = Array.isArray(auth) ? auth[0] : auth;
          const apiKeyHeader = Array.isArray(keyHeader) ? keyHeader[0] : keyHeader;
          const bearer = token?.startsWith("Bearer ") ? token.slice(7) : undefined;
          const provided = apiKeyHeader ?? bearer;
          if (!provided || provided !== apiKey) {
            res.writeHead(401, { "Content-Type": "application/json", ...corsHeaders });
            res.end(JSON.stringify({ error: "Invalid API key" }));
            return;
          }
        }
        const now = Date.now();
        const entry = rateTable.get(clientIp) ?? { count: 0, resetAt: now + rateLimit.windowMs };
        if (now > entry.resetAt) {
          entry.count = 0;
          entry.resetAt = now + rateLimit.windowMs;
        }

        const parsed = JSON.parse(body);
        if (Array.isArray(parsed)) {
          const requested = parsed.length;
          if (entry.count + requested > rateLimit.maxRequests) {
            res.writeHead(429, { "Content-Type": "application/json", ...corsHeaders });
            res.end(JSON.stringify({ error: "Rate limit exceeded" }));
            return;
          }
          entry.count += requested;
          rateTable.set(clientIp, entry);
          const responses = await Promise.all(parsed.map((item) => handler(item)));
          res.writeHead(200, { "Content-Type": "application/json", ...corsHeaders });
          res.end(JSON.stringify(responses));
          return;
        }
        if (entry.count + 1 > rateLimit.maxRequests) {
          res.writeHead(429, { "Content-Type": "application/json", ...corsHeaders });
          res.end(JSON.stringify({ error: "Rate limit exceeded" }));
          return;
        }
        entry.count += 1;
        rateTable.set(clientIp, entry);
        const response = await handler(parsed);
        res.writeHead(200, {
          "Content-Type": "application/json",
          ...corsHeaders,
        });
        res.end(JSON.stringify(response));
      } catch (error) {
        res.writeHead(500, {
          "Content-Type": "application/json",
          ...corsHeaders,
        });
        res.end(
          JSON.stringify({
            jsonrpc: "2.0",
            id: null,
            error: { code: -32000, message: error instanceof Error ? error.message : "Server error" },
          })
        );
      }
    });
  });

  server.listen(port, host);
  return server;
}
