# SpaceKit Network Whitepaper

This folder contains the SpaceKit technical whitepaper and historical research
materials. There is one current publication:

## Read

- **Canonical whitepaper**: [`SpaceKit-Whitepaper.md`](./SpaceKit-Whitepaper.md)
- **Canonical economics**:
  [`../../economics/spacekit-tokenomics/`](../../economics/spacekit-tokenomics/)

## Document status

The following files are retained for historical research and comparison. They
are not current product, deployment, or economic specifications:

- `Index.md`, `SWTCH-Whitepaper.md`, and the modular SWTCH chapters
- `EXECUTIVE_SUMMARY_2025.md`, `PRODUCTION_STATUS_SUMMARY.md`, and
  `REVOLUTIONARY_ACHIEVEMENTS_2025.md`
- `Tokenomics.md`, `SpaceKit-ASTRA-Fact-Sheet.md`, and `SpaceKit-PitchDeck.md`
- `WHITEPAPER_ANALYSIS.md`

Where any historical file conflicts with the canonical whitepaper or
`economics/spacekit-tokenomics`, the current canonical source wins.

## Build the PDF

The PDF build script compiles `SpaceKit-Whitepaper.md` using Pandoc + XeLaTeX.

```bash
cd spacekit-whitepaper
./compile-wp-pdf.sh
```