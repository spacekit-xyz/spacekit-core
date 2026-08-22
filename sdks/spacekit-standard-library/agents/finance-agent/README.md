# SpaceKit Financial Analysis Agent

## Overview
The **Financial Analysis Agent** is a SpaceKit/Growformer smart contract that exposes a compact binary protocol for financial computation. It mirrors the architectural patterns of RouteKit — routing, vault metering, remote storage refs, and Growformer‑backed reasoning — but specializes in **market data enrichment**, **risk analysis**, **factor modeling**, **sentiment scoring**, and **backtest‑style scenario evaluation**.

This agent is designed as a **foundation layer** for higher‑order financial agents, dashboards, quant pipelines, and automated risk monitors.

---

## 🧩 Capabilities
- **Market Snapshot** — normalized price/volume snapshot for a universe  
- **Risk Metrics** — VaR, Greeks, exposures, scenario summaries  
- **Factor Exposure** — value/size/momentum/quality loadings  
- **Sentiment Signal** — news‑driven sentiment scoring  
- **Backtest Query** — strategy spec → historical data → JSON summary  
- **Configure** — store portfolios/strategies as remote refs  
- **Brain Info** — Growformer model metadata  
- **Health** — operational status  

---

## 🔌 Wire Format (Little‑Endian u16)
All messages follow the same pattern as RouteKit:

```
[u8 opcode][u16 len][blob]...
```

### Opcodes

| Operation | Code | Payload |
|----------|------|---------|
| HEALTH | `0x10` | (empty) |
| CONFIGURE | `0x20` | `[prefs_len][prefs_utf8]` → `[ref_len][ref_utf8]` |
| BRAIN_INFO | `0x12` | (empty) |
| MARKET_SNAPSHOT | `0x30` | `[universe_len][universe_utf8]` |
| RISK_METRICS | `0x31` | `[portfolio_ref_len][ref][params_len][params]` |
| FACTOR_EXPOSURE | `0x32` | `[portfolio_ref_len][ref]` |
| SENTIMENT_SIGNAL | `0x33` | `[universe_len][universe_utf8]` |
| BACKTEST_QUERY | `0x34` | `[spec_len][spec_utf8]` |

---

## 💰 Vault Costs
Each opcode charges the caller’s vault:

- Market snapshot → `COST_DATA`
- Risk metrics → `COST_RISK`
- Factor exposure → `COST_FACTOR`
- Sentiment signal → `COST_SENTIMENT`
- Backtest query → `COST_BACKTEST`

This mirrors RouteKit’s economic model:

> “`payment_vault_charge(COST_LOCAL, beneficiary().as_str())?;`”  
> “`payment_vault_charge(COST_SEARCH_AND_LOCAL, ...)`”

---

## 🗄 Remote Storage Refs
The agent uses the same ref‑based pattern as RouteKit:

> “`let new_ref = remote_storage_put(new_transcript.as_bytes(), REF_OUT_MAX)?;`”

Refs allow:

- Portfolios  
- Strategy configs  
- User preferences  
- Backtest specs  

to be stored once and reused across multiple calls.

---

## 📡 Events
Every major operation emits an event with payload size for observability:

- `financial.market_snapshot`
- `financial.risk`
- `financial.factor`
- `financial.sentiment`
- `financial.backtest`

This mirrors RouteKit’s event pattern:

> “`emit_event_bytes("routekit.pipeline", &(out.len() as u32).to_le_bytes());`”

---

# 🧠 What the **financial_brain** must be trained on  
*(This is the real foundation — the model behind the opcodes.)*

The `financial_brain` is not a general LLM.  
It is a **Growformer‑style structured reasoning model** trained on **paired financial artifacts**.

Below is the dataset architecture you want.

---

## 1. **Portfolio → Risk → Explanation triplets**
This is the backbone.

Each training example is:

- **Input A:** Portfolio JSON  
- **Input B:** Risk parameters (horizon, confidence, model)  
- **Output:**  
  - Risk JSON (VaR, ES, Greeks, exposures)  
  - Natural‑language explanation  

This mirrors the RouteKit pattern:

> “`let prompt = format!("{transcript}\nUser: {msg}\nAssistant:");`”

But instead of chat, it’s **portfolio → risk → explanation**.

---

## 2. **Universe → Market Data → Normalized Snapshot**
Examples include:

- Universe spec (tickers, filters)  
- Raw market data (OHLCV, depth, fundamentals)  
- Normalized JSON snapshot  

This trains the model to **clean, align, and summarize** data.

---

## 3. **Portfolio → Factor Model → Loadings**
Training pairs include:

- Portfolio JSON  
- Factor covariance matrix  
- Factor exposures per instrument  
- Aggregate exposures  
- Explanation of what the exposures imply  

This is essential for:

- `FACTOR_EXPOSURE`
- Risk decomposition
- Attribution

---

## 4. **News Snippets → Sentiment → JSON Score**
This is where sentiment lives — not foundational, but a **signal engine**.

Training examples:

- News headlines  
- Short articles  
- Earnings call excerpts  
- Social sentiment summaries  

Outputs:

- Sentiment score (−1 to +1)  
- Volatility impact estimate  
- Event classification (earnings, M&A, regulatory, macro)  
- Short explanation  

This mirrors RouteKit’s pipeline pattern:

> “`Web results (JSON):\n{hits}\n\n---\nUser question:\n{uq}`”

---

## 5. **Backtest Spec → Historical Data → Metrics**
Training examples include:

- Strategy spec (rules, universe, parameters)  
- Historical data slice  
- Output JSON:
  - CAGR  
  - Max drawdown  
  - Sharpe  
  - Turnover  
  - Regime notes  

This is the backbone for `BACKTEST_QUERY`.

---

## 6. **Compliance & Guardrails**
The brain must be trained on:

- “Not investment advice” patterns  
- Risk disclosure templates  
- Uncertainty quantification  
- Scenario framing  

This ensures outputs are **safe, non‑prescriptive, and professional**.

---

# 🧬 Summary: What the financial_brain *is*
A **Growformer model** trained on:

- Structured financial schemas  
- Paired input/output financial tasks  
- Market + fundamentals + news  
- Risk/factor/backtest reasoning traces  
- Compliance‑safe narrative patterns  

It is **not** a prediction engine.  
It is a **reasoning engine over structured financial artifacts**.

---

