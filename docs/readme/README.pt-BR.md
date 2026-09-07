<p align="center"><img src="../../layerx-network.png" alt="LayerX" width="720"></p>

<h1 align="center">LayerX</h1>

Uma rede determinística de execução e contabilização para agentes autônomos.

[English](../../README.md) · [Español](README.es.md) · [日本語](README.ja.md) · [Русский](README.ru.md) · [简体中文](README.zh-CN.md) · Português · [Deutsch](README.de.md) · [Français](README.fr.md)

*Quando as versões diferem, o README em inglês é a referência.*

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](../../LICENSE)
[![CI](https://github.com/Sidiora-Labs/LayerX-Protocol/actions/workflows/ci.yml/badge.svg)](../../.github/workflows/ci.yml)

## O que é o LayerX

LayerX é uma rede determinística de execução e contabilização para agentes autônomos. Toda operação que altera estado entra como um `Activity` assinado e canonicamente codificado. O protocolo verifica o ator e sua autoridade, consome a sequência da conta, ordena a atividade em uma sequência global, aplica uma transição de estado determinística e devolve um recibo assinado vinculado à raiz de estado resultante.

O log de atividades somente de acréscimo é a autoridade. Os índices de banco de dados são projeções descartáveis e podem ser reconstruídos reproduzindo esse log. A execução crítica para consenso exclui ponto flutuante, decisões de relógio local, ordem de iteração de banco de dados e outras fontes de não determinismo. `402LXP` é o único componente autorizado a gravar saldos. Os módulos do protocolo emitem conjuntos de transferência validados em vez de alterar fundos por conta própria.

A atividade ordinária de agentes é executada e ordenada dentro do LayerX. Checkpoints periódicos liquidam no Paxeer, que detém a custódia, o registro de checkpoints, as cauções de avalistas, os desafios, os saques, as disputas e as saídas de emergência. Uma ação ordinária no LayerX não exige uma transação no Paxeer.

Este repositório é o monorepo da Sidiora Labs para o LayerX e a Paxeer Network. A colocalização mantém o protocolo, a rede de liquidação, os contratos e as superfícies de desenvolvedor auditáveis em um só lugar. Cada subsistema conserva seu próprio build, release, implantação e fronteira de confiança. Consulte [`spec/layerx-protocol/design.md`](../../spec/layerx-protocol/design.md).

## Experimente a testnet

O caminho completo está em [`docs/wiki/Quickstart.md`](../wiki/Quickstart.md): instale o CLI `layerx` a partir de `platform/cli`, suba o cluster, carregue `build/beta-cluster/env` com source, depois crie uma credencial, reivindique no faucet, envie uma atividade, verifique o recibo e implante um programa.

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

## Compilar a partir do código-fonte

O runtime principal é C17 (`-std=c17` no `Makefile` da raiz). Os workspaces agent, human e platform usam Rust 1.91.1 (`rust-toolchain.toml`). Os contratos de liquidação do LayerX usam Solidity 0.8.27 (`foundry.toml`). A qualificação de replay exige GCC 13, Clang 18, Docker, um runner musl amd64 e um cross-compiler AArch64 mais QEMU; consulte [`docs/QUALIFICATION.md`](../QUALIFICATION.md).

```sh
make build
make test
make test-contracts
make ci
```

Alvos delimitados do Paxeer, sem mudar de diretório:

```sh
make paxeer-build
make paxeer-lint
make paxeer-test
make paxeer-ci
```

`make ci` executa `public-audit`, testes nativos, uma comparação de arquivos de dois builds, verificações de símbolos de consenso e suítes de sanitizer. `make monorepo-ci` é um gate separado entre subsistemas. Uma passagem local não autoriza implantar contratos, mover custódia ou lidar com ativos reais.

## Estrutura do repositório

| Caminho | Função |
| --- | --- |
| `src/`, `include/` | Runtime do protocolo em C17, máquina de estados, armazenamento, sequenciamento, replay e integração de liquidação |
| `cmd/` | Daemons e ferramentas nativos (`layerxd`, `layerxctl`, genesis, verify) |
| `agent/` | Interface de agente em Rust, SDK, daemon, servidor MCP, encoding, criptografia e verificação de provas |
| `human/` | Plano de controle humano, compilador de intenção tipada, cliente na fronteira de custódia, índice do explorer e aplicação web |
| `platform/` | Plataforma de desenvolvedor, serviços hospedados, middleware, SDKs, emulador, CLI e ferramentas de release |
| `programs/` | Runtime programável do LayerX e ferramentas de programas |
| `interop/` | Superfícies de comércio entre agentes e interoperabilidade entre redes |
| `contracts/` | Contratos Solidity para custódia no Paxeer, checkpoints, bonding de avalistas, claims, disputas e saídas |
| `paxeer-network/` | Nó da Paxeer Network, compatibilidade EVM/RPC, engines de armazenamento, módulos, contratos e builds locais do subsistema |
| `spec/` | Especificações KVX normativas, designs gerados, requisitos e grafos de tarefas |
| `tests/`, `test/`, `fuzz/` | Suítes nativas, de contratos, replay, invariantes, falhas e fuzz |
| `migrations/` | Trabalho de genesis, migração, reconciliação e shadow-replay |
| `docs/` | Wiki, notas do monorepo e documentação de qualificação |

## Documentação

- Índice da wiki: [`docs/wiki/Home.md`](../wiki/Home.md)
- Layout do monorepo e tags de release: [`docs/MONOREPO.md`](../MONOREPO.md)
- Gates de qualificação: [`docs/QUALIFICATION.md`](../QUALIFICATION.md)
- Especificações: [`spec/`](../../spec/)

## SDKs e integrações

| Linguagem | Caminho |
| --- | --- |
| Rust | `agent/crates/layerx-sdk` |
| Python | `agent/sdk/python` |
| TypeScript | `agent/sdk/typescript` |
| Go | `platform/sdk/go` |
| JVM | `platform/sdk/jvm` |
| .NET | `platform/sdk/dotnet` |
| Swift | `platform/sdk/swift` |

`layerx-agentd` é `agent/crates/layerx-agentd`. O servidor MCP é `agent/crates/layerx-mcp`. O CLI de desenvolvedor em `platform/cli` instala esses transportes com `layerx install mcp` e `layerx install a2a`.

## Contribuindo

Leia [`CONTRIBUTING.md`](../../CONTRIBUTING.md) antes de abrir um pull request. Mudanças no protocolo começam em `spec/`. Não divulgue uma vulnerabilidade suspeita em uma issue pública; siga [`SECURITY.md`](../../SECURITY.md).

## Segurança

Reporte vulnerabilidades pelo relatório privado do GitHub, conforme descrito em [`SECURITY.md`](../../SECURITY.md).

## Licença

Licenciado sob a Licença Apache, Versão 2.0. Consulte [`LICENSE`](../../LICENSE) e [`NOTICE`](../../NOTICE).

LayerX é desenvolvido pela Sidiora Labs.
