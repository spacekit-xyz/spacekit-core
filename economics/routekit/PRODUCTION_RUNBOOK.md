# RouteKit P0 Production Runbook

P0 is an authenticated completion relay. It does not expose intent, vault, charge,
activity, compute-forwarding, or public metrics routes.

## Required services and secrets

- One reachable SpaceKit Storage Node with WAL enabled and protected on a private
  service network.
- At least one provider credential supplied by the deployment secret manager.
- A unique `ROUTEKIT_OPERATOR_DID`.
- API keys provisioned with `scripts/provision-api-key.sh`.
- TLS termination, request filtering, and origin enforcement at the public edge.

Start from `.env.production.example`. Never set `ROUTEKIT_BOOTSTRAP_KEYS` in
production.

## Deployment gate

1. CI format, test, clippy, production dependency audit, and image build pass.
2. Deploy Storage Node and verify its `/health` endpoint.
3. Provision a staging API key and store its plaintext value in the tenant secret
   manager only.
4. Start RouteKit. `/health` must return 200 and `/ready` must return 200.
5. Verify `/v1/intent`, `/v1/charge`, `/v1/charge-intent`, `/v1/activity/*`, and
   public `/metrics` return 404.
6. Verify a missing or invalid Bearer key returns 401.
7. Send one completion and confirm a `routekit-completions` document appears in
   Storage Node after the stream closes.
8. Restart RouteKit and confirm the API key remains valid and the prior receipt
   remains queryable.
9. Simulate a primary-provider 5xx and confirm the configured secondary provider
   serves the request.

## Monitoring

Scrape `ROUTEKIT_METRICS_ADDR/internal/metrics` from the private network. Alert on:

- readiness failures;
- provider 5xx or timeout bursts;
- authentication failures and rate-limit rejections;
- Storage Node write failures;
- completion error rate and latency;
- spend and token anomalies.

Logs must never contain prompts, API keys, or provider credentials. Correlate
requests with `x-request-id`.

## Incident actions

### Provider outage

Remove the failed provider credential from the deployment secret set and restart.
Readiness remains healthy only when another provider is configured.

### Storage Node outage

Do not bypass authentication. `/ready` returns 503 and the load balancer must drain
the instance. Cached keys may finish already-admitted requests, but new uncached
keys fail closed.

### Suspected API-key compromise

Set the key document's `enabled` field to `false`, wait one authentication-cache
TTL, and rotate the tenant key. For immediate revocation, restart RouteKit after
disabling the record.

### Unexpected spend

Remove provider credentials or scale RouteKit to zero. Preserve logs and completion
receipts for investigation. Do not enable unauthenticated bootstrap mode.

## Rollback

1. Keep the prior immutable image digest available.
2. Shift traffic away from the new image.
3. Deploy the prior digest with the same Storage Node and secret references.
4. Confirm `/ready`, authenticated completion, disabled-route 404s, and receipt
   persistence.
5. Restore traffic gradually.

P0 introduces no Storage Node schema migration, so image rollback does not require a
data rollback. The rollback target is under five minutes.
