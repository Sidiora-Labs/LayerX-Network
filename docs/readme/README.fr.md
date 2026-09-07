<p align="center"><img src="../../layerx-network.png" alt="LayerX" width="720"></p>

<h1 align="center">LayerX</h1>

Un réseau déterministe d'exécution et de comptabilité pour agents autonomes.

[English](../../README.md) · [Español](README.es.md) · [日本語](README.ja.md) · [Русский](README.ru.md) · [简体中文](README.zh-CN.md) · [Português](README.pt-BR.md) · [Deutsch](README.de.md) · Français

*En cas de divergence, le README anglais est la version de référence.*

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](../../LICENSE)
[![CI](https://github.com/Sidiora-Labs/LayerX-Network/actions/workflows/ci.yml/badge.svg)](../../.github/workflows/ci.yml)

## Ce qu'est LayerX

LayerX est un réseau déterministe d'exécution et de comptabilité pour agents autonomes. Toute opération qui modifie l'état entre sous la forme d'une `Activity` signée et encodée de façon canonique. Le protocole vérifie l'acteur et son autorité, consomme la séquence du compte, ordonne l'activité sur une séquence globale unique, applique une transition d'état déterministe, et renvoie un reçu signé lié à la racine d'état résultante.

Le journal d'activités en ajout seul fait autorité. Les index de base de données sont des projections jetables et peuvent être reconstruits en rejouant ce journal. L'exécution critique pour le consensus exclut les flottants, les décisions d'horloge locale, l'ordre d'itération des bases de données, et les autres sources de non-déterminisme. `402LXP` est le seul composant autorisé à écrire les soldes. Les modules du protocole émettent des ensembles de transferts validés plutôt que de muter eux-mêmes les fonds.

L'activité ordinaire des agents est exécutée et ordonnée dans LayerX. Des points de contrôle périodiques se règlent sur Paxeer, qui assure la garde, l'enregistrement des points de contrôle, les cautions de garants, les contestations, les retraits, les litiges et les sorties d'urgence. Une action LayerX ordinaire n'exige pas de transaction Paxeer.

Ce dépôt est le monorepo Sidiora Labs pour LayerX et le Paxeer Network. La colocalisation garde le protocole, le réseau de règlement, les contrats et les surfaces développeur auditables en un seul endroit. Chaque sous-système conserve sa propre compilation, publication, déploiement et frontière de confiance. Voir [`spec/layerx-protocol/design.md`](../../spec/layerx-protocol/design.md).

## Essayer le testnet

Le parcours complet est [`docs/wiki/Quickstart.md`](../wiki/Quickstart.md) : installer le CLI `layerx` depuis `platform/cli`, démarrer le cluster, sourcer `build/beta-cluster/env`, puis créer un identifiant, réclamer au faucet, soumettre une activité, vérifier le reçu, et déployer un programme.

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

## Compiler depuis les sources

Le runtime cœur est en C17 (`-std=c17` dans le `Makefile` racine). Les espaces de travail agent, human et platform utilisent Rust 1.91.1 (`rust-toolchain.toml`). Les contrats de règlement LayerX utilisent Solidity 0.8.27 (`foundry.toml`). La qualification par rejeu nécessite GCC 13, Clang 18, Docker, un exécuteur musl amd64, et un compilateur croisé AArch64 plus QEMU ; voir [`docs/QUALIFICATION.md`](../QUALIFICATION.md).

```sh
make build
make test
make test-contracts
make ci
```

Cibles bornées Paxeer, sans changer de répertoire :

```sh
make paxeer-build
make paxeer-lint
make paxeer-test
make paxeer-ci
```

`make ci` exécute `public-audit`, les tests natifs, une comparaison d'archives sur deux compilations, les contrôles de symboles de consensus, et les suites sanitizer. `make monorepo-ci` est une porte transversale distincte. Un succès local n'autorise pas à déployer des contrats, déplacer la garde, ou manipuler des actifs réels.

## Organisation du dépôt

| Chemin | Objet |
| --- | --- |
| `src/`, `include/` | Runtime protocole C17, machine d'état, stockage, séquencement, rejeu et intégration du règlement |
| `cmd/` | Démons et outils natifs (`layerxd`, `layerxctl`, genesis, verify) |
| `agent/` | Interface agent Rust, SDK, démon, serveur MCP, encodage, cryptographie et vérification de preuves |
| `human/` | Plan de contrôle humain, compilateur d'intentions typées, client de frontière de garde, index explorateur et application web |
| `platform/` | Plateforme développeur, services hébergés, intergiciels, SDK, émulateur, CLI et outillage de publication |
| `programs/` | Runtime LayerX programmable et outillage des programmes |
| `interop/` | Surfaces d'interopérabilité agent-commerce et inter-réseaux |
| `contracts/` | Contrats Solidity pour la garde Paxeer, les points de contrôle, le cautionnement des garants, les réclamations, les litiges et les sorties |
| `paxeer-network/` | Nœud Paxeer Network, compatibilité EVM/RPC, moteurs de stockage, modules, contrats et compilations locales au sous-système |
| `spec/` | Spécifications KVX normatives, conceptions générées, exigences et graphes de tâches |
| `tests/`, `test/`, `fuzz/` | Suites natives, contrats, rejeu, invariants, fautes et fuzz |
| `migrations/` | Travail de genèse, migration, réconciliation et rejeu parallèle |
| `docs/` | Wiki, notes du monorepo et documentation de qualification |

## Documentation

- Index du wiki : [`docs/wiki/Home.md`](../wiki/Home.md)
- Organisation du monorepo et étiquettes de publication : [`docs/MONOREPO.md`](../MONOREPO.md)
- Portes de qualification : [`docs/QUALIFICATION.md`](../QUALIFICATION.md)
- Spécifications : [`spec/`](../../spec/)

## SDK et intégrations

| Langage | Chemin |
| --- | --- |
| Rust | `agent/crates/layerx-sdk` |
| Python | `agent/sdk/python` |
| TypeScript | `agent/sdk/typescript` |
| Go | `platform/sdk/go` |
| JVM | `platform/sdk/jvm` |
| .NET | `platform/sdk/dotnet` |
| Swift | `platform/sdk/swift` |

`layerx-agentd` est `agent/crates/layerx-agentd`. Le serveur MCP est `agent/crates/layerx-mcp`. Le CLI développeur dans `platform/cli` installe ces transports avec `layerx install mcp` et `layerx install a2a`.

## Contribuer

Lire [`CONTRIBUTING.md`](../../CONTRIBUTING.md) avant d'ouvrir une pull request. Les changements de protocole commencent dans `spec/`. Ne pas divulguer une vulnérabilité suspectée dans une issue publique ; suivre [`SECURITY.md`](../../SECURITY.md).

## Sécurité

Signaler les vulnérabilités via le reporting privé GitHub, comme décrit dans [`SECURITY.md`](../../SECURITY.md).

## Licence

Distribué sous la licence Apache, version 2.0. Voir [`LICENSE`](../../LICENSE) et [`NOTICE`](../../NOTICE).

LayerX est développé par Sidiora Labs.
