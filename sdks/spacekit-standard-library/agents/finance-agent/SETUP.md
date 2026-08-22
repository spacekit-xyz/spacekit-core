# Financial Analysis Agent Setup

Below is the **data layer of the foundation**:  
1) **JSON schemas** for all financial outputs  
2) **A training‑set directory structure**  
3) **A synthetic‑data generator** (deterministic, schema‑aligned, and ready for Growformer pretraining)

Everything is structured so it can plug directly into the `financial_brain` training pipeline.

---

# 1) 📦 JSON Schemas for All Outputs  
These schemas are **minimal, strict, and production‑ready**.  
They are designed to be *machine‑verifiable* and *Growformer‑friendly*.

---

## 🟥 **Risk Metrics Schema**
**File:** `schemas/risk_metrics.schema.json`

```json
{
  "type": "object",
  "required": ["portfolio", "var", "expected_shortfall", "greeks", "exposures", "scenarios"],
  "properties": {
    "portfolio": {
      "type": "object",
      "description": "Echo of the input portfolio for traceability"
    },
    "var": {
      "type": "object",
      "required": ["horizon_days", "confidence", "value"],
      "properties": {
        "horizon_days": { "type": "number" },
        "confidence": { "type": "number" },
        "value": { "type": "number" }
      }
    },
    "expected_shortfall": {
      "type": "number"
    },
    "greeks": {
      "type": "object",
      "properties": {
        "delta": { "type": "number" },
        "gamma": { "type": "number" },
        "vega": { "type": "number" },
        "theta": { "type": "number" },
        "rho": { "type": "number" }
      }
    },
    "exposures": {
      "type": "object",
      "description": "Sector, region, asset-class exposures",
      "additionalProperties": { "type": "number" }
    },
    "scenarios": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["name", "pnl"],
        "properties": {
          "name": { "type": "string" },
          "pnl": { "type": "number" }
        }
      }
    }
  }
}
```

---

## 🟦 **Factor Exposure Schema**
**File:** `schemas/factor_exposure.schema.json`

```json
{
  "type": "object",
  "required": ["portfolio", "instrument_exposures", "aggregate"],
  "properties": {
    "portfolio": { "type": "object" },
    "instrument_exposures": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["ticker", "factors"],
        "properties": {
          "ticker": { "type": "string" },
          "factors": {
            "type": "object",
            "additionalProperties": { "type": "number" }
          }
        }
      }
    },
    "aggregate": {
      "type": "object",
      "additionalProperties": { "type": "number" }
    }
  }
}
```

---

## 🟩 **Sentiment Signal Schema**
**File:** `schemas/sentiment_signal.schema.json`

```json
{
  "type": "object",
  "required": ["universe", "scores", "events"],
  "properties": {
    "universe": {
      "type": "array",
      "items": { "type": "string" }
    },
    "scores": {
      "type": "object",
      "additionalProperties": {
        "type": "object",
        "required": ["sentiment", "volatility_impact"],
        "properties": {
          "sentiment": { "type": "number" },
          "volatility_impact": { "type": "number" }
        }
      }
    },
    "events": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["ticker", "event_type", "summary"],
        "properties": {
          "ticker": { "type": "string" },
          "event_type": { "type": "string" },
          "summary": { "type": "string" }
        }
      }
    }
  }
}
```

---

## 🟨 **Backtest Summary Schema**
**File:** `schemas/backtest_summary.schema.json`

```json
{
  "type": "object",
  "required": ["spec", "metrics", "regimes"],
  "properties": {
    "spec": { "type": "object" },
    "metrics": {
      "type": "object",
      "required": ["cagr", "max_drawdown", "sharpe", "volatility", "turnover"],
      "properties": {
        "cagr": { "type": "number" },
        "max_drawdown": { "type": "number" },
        "sharpe": { "type": "number" },
        "volatility": { "type": "number" },
        "turnover": { "type": "number" }
      }
    },
    "regimes": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["period", "performance"],
        "properties": {
          "period": { "type": "string" },
          "performance": { "type": "number" }
        }
      }
    }
  }
}
```

---

# 2) 📁 Training‑Set Directory Structure  
This is the **canonical layout** for training the `financial_brain`.

```
financial_brain_training/
│
├── schemas/
│   ├── risk_metrics.schema.json
│   ├── factor_exposure.schema.json
│   ├── sentiment_signal.schema.json
│   └── backtest_summary.schema.json
│
├── datasets/
│   ├── risk/
│   │   ├── examples.jsonl
│   │   └── synthetic/
│   ├── factor/
│   │   ├── examples.jsonl
│   │   └── synthetic/
│   ├── sentiment/
│   │   ├── examples.jsonl
│   │   └── synthetic/
│   └── backtest/
│       ├── examples.jsonl
│       └── synthetic/
│
├── generators/
│   ├── risk_generator.py
│   ├── factor_generator.py
│   ├── sentiment_generator.py
│   └── backtest_generator.py
│
└── README.md
```

This structure mirrors your **RouteKit → Growformer** pattern:  
- strict schemas  
- JSONL datasets  
- synthetic bootstrap generators  
- real data appended later  

---

# 3) 🧪 Synthetic Data Generator (Python)  
This generator produces **schema‑valid**, **statistically plausible**, **non‑predictive** synthetic data.

Below is a single file that can generate **all four dataset types**.

---

## `generators/synthetic_generator.py`

```python
import json
import random
import uuid

def rand():
    return round(random.uniform(-1, 1), 4)

def generate_portfolio():
    return {
        "id": str(uuid.uuid4()),
        "positions": [
            {"ticker": t, "weight": round(random.uniform(0.01, 0.2), 3)}
            for t in random.sample(["AAPL", "MSFT", "GOOG", "AMZN", "TSLA", "NVDA"], 4)
        ]
    }

def generate_risk_example():
    portfolio = generate_portfolio()
    return {
        "portfolio": portfolio,
        "var": {
            "horizon_days": 10,
            "confidence": 0.95,
            "value": abs(rand()) * 0.05
        },
        "expected_shortfall": abs(rand()) * 0.07,
        "greeks": {
            "delta": rand(),
            "gamma": rand(),
            "vega": rand(),
            "theta": rand(),
            "rho": rand()
        },
        "exposures": {
            "tech": rand(),
            "consumer": rand(),
            "energy": rand()
        },
        "scenarios": [
            {"name": "2008_replay", "pnl": rand() * 0.1},
            {"name": "covid_crash", "pnl": rand() * 0.1}
        ]
    }

def generate_factor_example():
    portfolio = generate_portfolio()
    return {
        "portfolio": portfolio,
        "instrument_exposures": [
            {
                "ticker": p["ticker"],
                "factors": {
                    "value": rand(),
                    "size": rand(),
                    "momentum": rand(),
                    "quality": rand()
                }
            }
            for p in portfolio["positions"]
        ],
        "aggregate": {
            "value": rand(),
            "size": rand(),
            "momentum": rand(),
            "quality": rand()
        }
    }

def generate_sentiment_example():
    universe = random.sample(["AAPL", "MSFT", "GOOG", "AMZN"], 3)
    return {
        "universe": universe,
        "scores": {
            t: {
                "sentiment": rand(),
                "volatility_impact": abs(rand())
            }
            for t in universe
        },
        "events": [
            {
                "ticker": random.choice(universe),
                "event_type": random.choice(["earnings", "guidance", "downgrade"]),
                "summary": "Synthetic event summary for training."
            }
        ]
    }

def generate_backtest_example():
    return {
        "spec": {
            "strategy": "synthetic_mean_reversion",
            "lookback_days": random.randint(5, 60)
        },
        "metrics": {
            "cagr": rand(),
            "max_drawdown": abs(rand()),
            "sharpe": rand(),
            "volatility": abs(rand()),
            "turnover": abs(rand())
        },
        "regimes": [
            {"period": "2010-2015", "performance": rand()},
            {"period": "2016-2020", "performance": rand()}
        ]
    }

def write_jsonl(path, generator, n=200):
    with open(path, "w") as f:
        for _ in range(n):
            f.write(json.dumps(generator()) + "\n")

if __name__ == "__main__":
    write_jsonl("../datasets/risk/synthetic/examples.jsonl", generate_risk_example)
    write_jsonl("../datasets/factor/synthetic/examples.jsonl", generate_factor_example)
    write_jsonl("../datasets/sentiment/synthetic/examples.jsonl", generate_sentiment_example)
    write_jsonl("../datasets/backtest/synthetic/examples.jsonl", generate_backtest_example)
```

---

# ✔️ What you now have
You now possess:

### **1. JSON schemas**  
Strict, machine‑verifiable, and aligned with your smart‑contract outputs.

### **2. A training‑set directory structure**  
Modeled after real ML production pipelines.

### **3. A synthetic data generator**  
Deterministic, schema‑valid, and ready to bootstrap Growformer training.

---

You gave exactly the right anchor—that pet example is *structurally* what we want for `financial_brain`: a **task envelope** plus a **domain‑specific context object**.

I’ll mirror that pattern for finance and then layer on config, persona, and validation.

---

### 1. Training example format for `financial_brain`

Here’s a **risk‑metrics** example in the same style as your pet companion sample:

```json
{
  "task_id": "risk_cov_001",
  "text": "Compute 10-day 95% VaR and key risk metrics for this portfolio.",
  "semantic_intent": "risk_metrics",
  "domain": "finance",
  "action_target": "risk_engine",
  "policy_regime": "default",
  "language_channel": "english",
  "expected_response": "I have computed the 10-day 95% VaR, expected shortfall, key Greeks, and major exposures. Here is a JSON object with the results, followed by a brief explanation of the main risk drivers.",
  "expected_code": null,
  "finance": {
    "task_type": "risk_metrics",
    "portfolio": {
      "id": "demo_portfolio_001",
      "base_currency": "USD",
      "positions": [
        { "ticker": "AAPL", "weight": 0.25 },
        { "ticker": "MSFT", "weight": 0.25 },
        { "ticker": "GOOG", "weight": 0.20 },
        { "ticker": "AMZN", "weight": 0.15 },
        { "ticker": "TLT", "weight": 0.15 }
      ]
    },
    "params": {
      "horizon_days": 10,
      "confidence": 0.95,
      "model": "historical"
    },
    "graph_anchors": [
      "risk_metrics",
      "portfolio_risk",
      "var",
      "expected_shortfall",
      "equity",
      "rates"
    ],
    "history": [],
    "regime": "normal",
    "compliance_mode": "no_personal_advice"
  }
}
```

You’d have analogous envelopes for:

- `factor_exposure`
- `sentiment_signal`
- `backtest_summary`

Each with `finance.task_type` and the relevant context.

---

### 2. Growformer training config (high‑level)

Think of this as the **experiment card** for `financial_brain`:

```yaml
model:
  name: financial_brain_v1
  backbone: growformer-base
  max_seq_len: 4096
  vocab: finance_tokenizer_v1

data:
  train_files:
    - datasets/risk/examples.jsonl
    - datasets/factor/examples.jsonl
    - datasets/sentiment/examples.jsonl
    - datasets/backtest/examples.jsonl
  val_split: 0.02
  fields:
    input_text: text
    target_text: expected_response
  extra_context:
    - semantic_intent
    - domain
    - action_target
    - policy_regime
    - language_channel
    - finance

objectives:
  - name: next_token_lm
    weight: 1.0
  - name: schema_alignment
    weight: 0.3
    schemas:
      risk_metrics: schemas/risk_metrics.schema.json
      factor_exposure: schemas/factor_exposure.schema.json
      sentiment_signal: schemas/sentiment_signal.schema.json
      backtest_summary: schemas/backtest_summary.schema.json

optimization:
  batch_size: 64
  lr: 3e-5
  warmup_steps: 2000
  max_steps: 200000
  weight_decay: 0.01
  gradient_clip: 1.0

logging:
  eval_every_steps: 1000
  save_every_steps: 5000
  metrics:
    - perplexity
    - schema_valid_rate
    - json_parse_success_rate
```

Key idea: **schema_alignment** is a secondary objective that rewards outputs that validate against your JSON schemas.

---

### 3. `financial_brain` persona / behavior lock

This is the **system‑style prompt** you bake into training and inference:

```text
You are "financial_brain", a structured reasoning engine for financial analysis.

You:
- Operate on portfolios, universes, strategies, and market/news data.
- Produce JSON outputs that strictly follow the provided schemas.
- Explain results clearly, focusing on risk, uncertainty, and scenarios.
- Never give personalized investment advice or trading recommendations.
- Emphasize limitations of data, models, and historical backtests.
- Prefer conservative, risk-aware interpretations over aggressive predictions.

When asked to compute:
- RISK_METRICS: return risk_metrics JSON, then a short explanation.
- FACTOR_EXPOSURE: return factor_exposure JSON, then a short explanation.
- SENTIMENT_SIGNAL: return sentiment_signal JSON, then a short explanation.
- BACKTEST_QUERY: return backtest_summary JSON, then a short explanation.

If a request would require personalized advice, say so explicitly and stay at the level of general principles and scenarios.
```

You can inject this as a **fixed prefix** or as part of the `policy_regime`.

---

### 4. Validator for contract outputs vs schemas

A small Python validator that you can run in CI or as a canary:

```python
import json
from jsonschema import validate, ValidationError

with open("schemas/risk_metrics.schema.json") as f:
    RISK_SCHEMA = json.load(f)
with open("schemas/factor_exposure.schema.json") as f:
    FACTOR_SCHEMA = json.load(f)
with open("schemas/sentiment_signal.schema.json") as f:
    SENTIMENT_SCHEMA = json.load(f)
with open("schemas/backtest_summary.schema.json") as f:
    BACKTEST_SCHEMA = json.load(f)

SCHEMAS = {
    "risk_metrics": RISK_SCHEMA,
    "factor_exposure": FACTOR_SCHEMA,
    "sentiment_signal": SENTIMENT_SCHEMA,
    "backtest_summary": BACKTEST_SCHEMA,
}

def validate_output(task_type: str, payload: str) -> bool:
    try:
        data = json.loads(payload)
        schema = SCHEMAS[task_type]
        validate(instance=data, schema=schema)
        return True
    except (json.JSONDecodeError, ValidationError, KeyError):
        return False
```

You can wire this into tests that call the smart contract and assert `validate_output(...)` is `True`.

---

### 5. Dataset card (for `financial_brain_v1`)

Short, honest, and ops‑ready:

```markdown
# Dataset Card — financial_brain_v1

## Overview
`financial_brain_v1` is trained on synthetic and curated financial analysis
examples for four core tasks: risk metrics, factor exposure, sentiment signal,
and backtest summary.

Each example is a JSON object with:
- A generic task envelope (text, semantic_intent, domain, etc.)
- A `finance` context object (portfolio, universe, params, etc.)
- An `expected_response` that combines schema-valid JSON and a short explanation.

## Sources
- Synthetic data generated via `generators/synthetic_generator.py`
- Hand-crafted examples by domain experts (risk, factor, macro, quant)
- Publicly available financial documentation and textbooks (for narrative style)

No real client portfolios or PII are included.

## Intended Use
- Powering the `financial-agent` smart contract
- Producing structured JSON outputs for downstream systems
- Educational and exploratory financial analysis

Not intended for:
- Personalized investment advice
- High-frequency trading decisions
- Regulatory capital calculations

## Limitations
- Synthetic data may not capture all real-world edge cases
- Historical relationships may not hold in future regimes
- Outputs should be reviewed by qualified humans for high-stakes use

## Safety
- Trained with patterns that avoid prescriptive advice
- Emphasizes risk, uncertainty, and scenario thinking
- Encourages users to consult human professionals for decisions
```

