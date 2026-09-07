<p align="center"><img src="../../layerx-network.png" alt="LayerX" width="720"></p>

<h1 align="center">LayerX</h1>

Ein deterministisches Ausführungs- und Buchungsnetzwerk für autonome Agenten.

[English](../../README.md) · [Español](README.es.md) · [日本語](README.ja.md) · [Русский](README.ru.md) · [简体中文](README.zh-CN.md) · [Português](README.pt-BR.md) · Deutsch · [Français](README.fr.md)

*Weicht diese Fassung von der englischen README ab, gilt die englische Fassung als Referenz.*

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](../../LICENSE)
[![CI](https://github.com/Sidiora-Labs/LayerX-Network/actions/workflows/ci.yml/badge.svg)](../../.github/workflows/ci.yml)

## Was LayerX ist

LayerX ist ein deterministisches Ausführungs- und Buchungsnetzwerk für autonome Agenten. Jede zustandsändernde Operation geht als signierte, kanonisch kodierte `Activity` ein. Das Protokoll prüft den Akteur und seine Berechtigung, verbraucht die Kontosequenz, reiht die Activity in eine globale Sequenz ein, wendet einen deterministischen Zustandsübergang an und gibt eine signierte Quittung zurück, die an die resultierende State Root gebunden ist.

Das nur anfügende Aktivitätsprotokoll ist die Autorität. Datenbankindizes sind verwerfbare Projektionen und lassen sich durch Replay dieses Protokolls neu aufbauen. Konsenskritische Ausführung schließt Gleitkommaarithmetik, Entscheidungen anhand der lokalen Uhr, die Iterationsreihenfolge von Datenbanken und andere Quellen von Nichtdeterminismus aus. `402LXP` ist die einzige Komponente, die Salden schreiben darf. Protokollmodule geben validierte Transfersätze aus, statt Guthaben selbst zu verändern.

Gewöhnliche Agentenaktivität wird innerhalb von LayerX ausgeführt und geordnet. Periodische Checkpoints werden auf Paxeer abgewickelt, das Verwahrung, Checkpoint-Registrierung, Garantenbonds, Challenges, Auszahlungen, Streitfälle und Notausstiege hält. Eine gewöhnliche LayerX-Aktion erfordert keine Paxeer-Transaktion.

Dieses Repository ist das Monorepo von Sidiora Labs für LayerX und das Paxeer Network. Die gemeinsame Ablage hält Protokoll, Settlement-Netzwerk, Contracts und Entwickleroberflächen an einem Ort prüfbar. Jedes Subsystem behält seine eigene Build-, Release-, Deployment- und Vertrauensgrenze. Siehe [`spec/layerx-protocol/design.md`](../../spec/layerx-protocol/design.md).

## Testnet ausprobieren

Der vollständige Pfad steht in [`docs/wiki/Quickstart.md`](../wiki/Quickstart.md): die `layerx` CLI aus `platform/cli` installieren, den Cluster hochfahren, `build/beta-cluster/env` sourcen, dann ein Credential anlegen, vom Faucet beanspruchen, eine Activity einreichen, die Quittung prüfen und ein Programm deployen.

```sh
layerx key create quickstart
```

```sh
jq -n --arg did "$LAYERX_TEST_SOURCE_DID" --arg public_key "$LAYERX_TEST_SOURCE_PUBLIC_KEY" \
  '{did:$did, public_key:$public_key}' > faucet-request.json
curl --fail --silent --show-error --max-time 30 --cacert "$LAYERX_TEST_CA_FILE" \
  --header "Authorization: Bearer $(tr -d '\r\n' < "$LAYERX_TEST_AUTH_TOKEN_FILE")" \
  --request POST "$LAYERX_FAUCET_URL/v1/faucet/claims" \
  --header "Idempotency-Key: faucet-quickstart-01" \
  --header 'Content-Type: application/json' --data-binary @faucet-request.json
```

```sh
layerx --json payment test \
  --from "$LAYERX_TEST_SOURCE_DID" \
  --to "$LAYERX_TEST_DESTINATION_DID" \
  --currency "$LAYERX_TEST_ASSET" \
  --amount "$LAYERX_TEST_AMOUNT" \
  --idempotency-key paymentquickstart1
```

```sh
layerx --json receipt verify \
  --receipt receipt.hex \
  --batch-id "$batch_id" \
  --asset "$asset" \
  --previous-state-root "$previous_root" \
  --resulting-state-root "$resulting_root" \
  --sequencer-public-key "$sequencer_key"
```

```sh
layerx --json program deploy \
  quickstart-program/target/wasm32-unknown-unknown/release/quickstart_program.wasm \
  --program-id <program_id> \
  --idempotency-key <idempotency_key> \
  --key quickstart \
  --account-sequence 0 \
  --not-before-ms <not_before_ms> \
  --expires-at-ms <expires_at_ms> \
  --previous-state-root <previous_state_root>
```

## Aus dem Quellcode bauen

Die Kernruntime ist C17 (`-std=c17` im Root-`Makefile`). Die Workspaces agent, human und platform nutzen Rust 1.91.1 (`rust-toolchain.toml`). LayerX-Settlement-Contracts nutzen Solidity 0.8.27 (`foundry.toml`). Replay-Qualifikation braucht GCC 13, Clang 18, Docker, einen amd64-musl-Runner sowie einen AArch64-Cross-Compiler plus QEMU; siehe [`docs/QUALIFICATION.md`](../QUALIFICATION.md).

```sh
make build
make test
make test-contracts
make ci
```

Paxeer-begrenzte Targets, ohne das Verzeichnis zu wechseln:

```sh
make paxeer-build
make paxeer-lint
make paxeer-test
make paxeer-ci
```

`make ci` führt `public-audit`, native Tests, einen Archivvergleich zweier Builds, Konsenssymbolprüfungen und Sanitizer-Suiten aus. `make monorepo-ci` ist ein separates, subsystemübergreifendes Gate. Ein lokales Bestehen ist keine Berechtigung, Contracts zu deployen, verwahrte Mittel zu bewegen oder reale Vermögenswerte zu handhaben.

## Repository-Aufbau

| Pfad | Zweck |
| --- | --- |
| `src/`, `include/` | C17-Protokollruntime, Zustandsmaschine, Speicher, Sequenzierung, Replay und Settlement-Integration |
| `cmd/` | Native Daemons und Werkzeuge (`layerxd`, `layerxctl`, genesis, verify) |
| `agent/` | Rust-Agentenschnittstelle, SDK, Daemon, MCP-Server, Kodierung, Kryptographie und Proof-Verifikation |
| `human/` | Menschliche Steuerungsebene, typisierter Intent-Compiler, Custody-Boundary-Client, Explorer-Index und Webanwendung |
| `platform/` | Entwicklerplattform, gehostete Dienste, Middleware, SDKs, Emulator, CLI und Release-Werkzeuge |
| `programs/` | Programmierbare LayerX-Runtime und Programmwerkzeuge |
| `interop/` | Agent-Commerce- und netzwerkübergreifende Interoperabilitätsoberflächen |
| `contracts/` | Solidity-Contracts für Paxeer-Verwahrung, Checkpoints, Garantenbonding, Claims, Streitfälle und Exits |
| `paxeer-network/` | Paxeer Network Node, EVM/RPC-Kompatibilität, Speicherengines, Module, Contracts und subsystemlokale Builds |
| `spec/` | Normative KVX-Spezifikationen, generierte Designs, Anforderungen und Task-Graphen |
| `tests/`, `test/`, `fuzz/` | Native, Contract-, Replay-, Invarianten-, Fault- und Fuzz-Suiten |
| `migrations/` | Genesis-, Migrations-, Abstimmungs- und Shadow-Replay-Arbeit |
| `docs/` | Wiki, Monorepo-Notizen und Qualifikationsdokumentation |

## Dokumentation

- Wiki-Index: [`docs/wiki/Home.md`](../wiki/Home.md)
- Monorepo-Aufbau und Release-Tags: [`docs/MONOREPO.md`](../MONOREPO.md)
- Qualifikationsgates: [`docs/QUALIFICATION.md`](../QUALIFICATION.md)
- Spezifikationen: [`spec/`](../../spec/)

## SDKs und Integrationen

| Sprache | Pfad |
| --- | --- |
| Rust | `agent/crates/layerx-sdk` |
| Python | `agent/sdk/python` |
| TypeScript | `agent/sdk/typescript` |
| Go | `platform/sdk/go` |
| JVM | `platform/sdk/jvm` |
| .NET | `platform/sdk/dotnet` |
| Swift | `platform/sdk/swift` |

`layerx-agentd` ist `agent/crates/layerx-agentd`. Der MCP-Server ist `agent/crates/layerx-mcp`. Die Entwickler-CLI in `platform/cli` installiert diese Transports mit `layerx install mcp` und `layerx install a2a`.

## Mitwirken

Vor dem Öffnen eines Pull Requests [`CONTRIBUTING.md`](../../CONTRIBUTING.md) lesen. Protokolländerungen beginnen in `spec/`. Eine vermutete Schwachstelle nicht in einem öffentlichen Issue offenlegen; [`SECURITY.md`](../../SECURITY.md) folgen.

## Sicherheit

Schwachstellen über GitHub Private Reporting melden, wie in [`SECURITY.md`](../../SECURITY.md) beschrieben.

## Lizenz

Lizenziert unter der Apache License, Version 2.0. Siehe [`LICENSE`](../../LICENSE) und [`NOTICE`](../../NOTICE).

LayerX wird von Sidiora Labs entwickelt.
