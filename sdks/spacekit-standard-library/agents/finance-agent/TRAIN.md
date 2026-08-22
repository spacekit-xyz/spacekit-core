# Financial Analysis Agent Training


---

### What is the `financial_brain` actually trained on?

If we’re honest, this is the crux: the contract is just a shell; the brain is the edge.

I’d treat `financial_brain` as a **specialized Growformer** trained on a blend of:

1. **Structured financial schemas and workflows**  
   - Portfolio JSON schemas (positions, instruments, constraints, benchmarks).  
   - Risk report schemas (VaR, Greeks, exposures, stress scenarios).  
   - Factor model schemas (factor names, loadings, covariance structures).  
   - Backtest result schemas (returns, drawdowns, turnover, attribution).

2. **Textual financial knowledge and practice**  
   - Risk management docs: how VaR, ES, stress testing, margining work.  
   - Portfolio management playbooks: rebalancing, constraints, mandate language.  
   - Factor investing literature: Fama–French, quality, momentum, low vol, etc.  
   - Execution and microstructure basics (enough to talk sensibly about slippage, liquidity, impact).

3. **Market and fundamentals context**  
   - Historical fundamentals (income statements, balance sheets, cash flows).  
   - Sector/industry taxonomies and typical risk profiles.  
   - Macro indicators and their usual interpretations (rates, inflation, growth).

4. **Sentiment and news patterns**  
   - News headlines and short articles mapped to **event types** (earnings beats/misses, guidance changes, M&A, downgrades, regulatory actions).  
   - Labeled sentiment / impact (e.g., “earnings beat + raised guidance → positive short‑term sentiment, medium‑term re‑rating risk”).  
   - Transcripts (earnings calls, investor days) with tone and forward‑looking statements.

5. **Reasoning traces over financial tasks**  
   - Step‑by‑step examples of:
     - Computing and explaining risk metrics from a portfolio blob.  
     - Explaining factor exposures and their implications.  
     - Interpreting a backtest result and pointing out overfitting / regime risk.  
     - Reconciling conflicting signals (e.g., positive sentiment, worsening fundamentals).

6. **Guardrails and compliance patterns**  
   - Clear patterns for:
     - Not giving individualized investment advice.  
     - Flagging uncertainty and data limitations.  
     - Emphasizing risk and scenario thinking over “predictions”.

In other words: it’s not just “finance text”; it’s **paired structure + narrative**:

- “Here is a portfolio JSON → here is the risk JSON → here is the explanation.”
- “Here is a universe + news snippets → here is a sentiment JSON → here is the rationale.”

That’s what lets the brain sit behind your opcodes and reliably turn:

- blobs from `remote_storage_get`  
- JSON from data gateways  

into **coherent, schema‑respecting outputs** that your downstream systems can trust.

