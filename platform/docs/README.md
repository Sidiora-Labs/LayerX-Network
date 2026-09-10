# LayerX developer documentation

`build/build_site.py` renders `site.kvx` into a static site under `site/`. It has no dependencies beyond Python 3.

```
python3 build/build_site.py ../..
python3 build/build_site.py --check ../..
```

The first form writes. The second asserts, and fails when a generated page or an extracted code block is stale.

## What is generated

`content/reference/human-api.md`, `content/reference/agent-api.md` and `content/reference/errors.md` are generated from `human/schema/human-api` and `agent/schema/agent-api`. `content/reference/enforcement.md` is generated from `capabilities.kvx`, and `content/reference/samples.md` from `samples.kvx`. None of the five is hand-edited; the build overwrites them.

Every other page under `content/` is written by hand and must carry an `Enforced by` table naming, for each capability it documents, the layer that enforces it: `protocol`, `agent-layer`, `service` or `hosted-surface`. A page without one fails the build.

`testnet.md` sits outside `content/` and is never rewritten.

Public wiki pages for payments (wallet, faucet, send, token, 402), the
OpenRPC method list, Asset encodings, and commitment levels live under
[`docs/wiki/`](../../docs/wiki/Home.md). Payment surfaces that are not in this
tree yet are named there.

## Samples

`samples.kvx` declares every sample directory in `samples/`. A code fence in a page carrying `sample=<id>` is filled from that sample's entry file; `file=` selects a different file in the directory, and `region=` selects a `layerx:begin <name>` / `layerx:end <name>` block within it. The fence language must match the language the sample declares.

Each sample declares a `measured_region` and a `maximum_integration_lines` budget. The build counts the non-blank lines between that region's markers and fails when any sample exceeds its budget, writing the counts to `site/measurements.json`.

Samples are real programs. They build and run against a LayerX environment - the local emulator by default - and each one names the environment it needs in `samples.kvx`.
