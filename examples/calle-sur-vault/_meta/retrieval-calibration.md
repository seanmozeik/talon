---
title: Retrieval Calibration Notes
type: meta
status: active
tags: [meta, retrieval, calibration, search, scopes]
last_updated: 2026-04-29
---

# Retrieval Calibration Notes

This note defines lightweight dogfood expectations for the Calle Sur vault. It is
not a full eval suite. It is a small set of queries that make scope priority
behavior visible while tuning search.

## Scope Shape

- `wiki/` is labeled boosted because compiled knowledge should be easy to find.
- `projects/` is labeled elevated because active work should win when the query
  is about current execution.
- All ranking multipliers are temporarily neutral (`1.0`) so search can be
  calibrated from equality instead of fighting old magic numbers.
- `raw/`, `daily/`, `archive/`, `private/`, and `_meta/` are excluded from the
  default search pool by `examples/config.toml`.

## Shouts Probe

The important failure mode is a broad wiki article outranking a more
specific active project note. In this vault, [[Sauce Mothers]] is the broad wiki
trap: it mentions fermented finishes and sauce foundations, but it is not the
current hot sauce launch plan.

Expected behavior:

| Query | Expected shape |
|-------|----------------|
| `fermented hot sauce line` | [[Fermented Hot Sauce Line]] and [[Launch Readiness]] should beat [[Sauce Mothers]]. |
| `hot sauce launch readiness` | [[Launch Readiness]] should be near the top. |
| `hot sauce formulation` | [[Hot Sauce Formulation]] should beat project notes. |
| `sauce mother fermented finish` | [[Sauce Mothers]] should be near the top. |
| `co packer hot sauce quote` | No raw email by default; include it with `--scope raw`. |
| `lease renewal landlord` | No private notes by default; include them with `--scope private`. |

## Suggested Commands

```bash
talon -c examples/config.toml --agent search "fermented hot sauce line"
talon -c examples/config.toml --agent search "hot sauce launch readiness"
talon -c examples/config.toml --agent search "hot sauce formulation"
talon -c examples/config.toml --agent search "sauce mother fermented finish"
talon -c examples/config.toml --agent search "co packer hot sauce quote"
talon -c examples/config.toml --agent search "co packer hot sauce quote" --scope raw
talon -c examples/config.toml --agent search "lease renewal landlord"
talon -c examples/config.toml --agent search "lease renewal landlord" --scope private
```

## Tuning Notes

When inspecting results, compare `score` and `rawScore`. During the neutral
baseline they should match. If future multipliers are reintroduced, a shout will
look like a lower `rawScore` result with a high final `score` because scope
priority fired too early.
