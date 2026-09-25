# Third-party references and development dependencies

Aeris Charts' product code, architecture, rendering backends, state model, and public API are
independently designed and implemented.

The project studies public documentation, public examples, and observable behavior from established
financial-charting products to understand familiar interaction conventions and to build
development-only comparison fixtures. References to those products describe research inputs or
compatibility targets; they do not indicate shared source code, affiliation, endorsement, or a
drop-in clone.

## Lightweight Charts

The browser test workspace pins
[Lightweight Charts](https://github.com/tradingview/lightweight-charts) 5.2.1 as a development-only
dependency. Tests call its public API in an isolated reference fixture and compare observable output
or interaction behavior with Aeris Charts. The dependency is not bundled into the published
`aeris-charts` package.

Some development-only comparison fixtures are derived from public plugin examples. They remain
isolated from product code and are covered by the upstream Apache License 2.0. The
applicable license text is included at
[`third_party_licenses/Apache-2.0.txt`](third_party_licenses/Apache-2.0.txt).

Copyright 2023 TradingView, Inc.

Lightweight Charts is licensed under the Apache License, Version 2.0.

## Trademarks

TradingView and Lightweight Charts are trademarks of their respective owners. Aeris Charts is
not affiliated with or endorsed by TradingView, Inc. Trademark names are used only for factual
attribution of research references and development dependencies.
