<p align="center"><img src="../../layerx-network.png" alt="LayerX" width="720"></p>

<h1 align="center">LayerX</h1>

Una red determinista de ejecución y contabilidad para agentes autónomos.

[English](../../README.md) · Español · [日本語](README.ja.md) · [Русский](README.ru.md) · [简体中文](README.zh-CN.md) · [Português](README.pt-BR.md) · [Deutsch](README.de.md) · [Français](README.fr.md)

*Cuando este archivo y el README en inglés difieren, el README en inglés es la versión de referencia.*

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](../../LICENSE)
[![CI](https://github.com/Sidiora-Labs/LayerX-Network/actions/workflows/ci.yml/badge.svg)](../../.github/workflows/ci.yml)

## Qué es LayerX

LayerX es una red determinista de ejecución y contabilidad para agentes autónomos. Toda operación que cambia el estado entra como un `Activity` firmado y codificado de forma canónica. El protocolo verifica al actor y su autoridad, consume la secuencia de la cuenta, ordena la actividad en una secuencia global, aplica una transición de estado determinista y devuelve un recibo firmado ligado a la raíz de estado resultante.

El registro de actividades de solo anexión es la autoridad. Los índices de base de datos son proyecciones desechables y se pueden reconstruir reejecutando ese registro. La ejecución crítica para el consenso excluye punto flotante, decisiones de reloj local, orden de iteración de la base de datos y otras fuentes de no determinismo. `402LXP` es el único componente autorizado a escribir saldos. Los módulos del protocolo emiten conjuntos de transferencias validados en lugar de mutar fondos por sí mismos.

La actividad ordinaria de los agentes se ejecuta y se ordena dentro de LayerX. Los puntos de control periódicos se liquidan en Paxeer, que retiene la custodia, el registro de puntos de control, las fianzas de garantes, los desafíos, los retiros, las disputas y las salidas de emergencia. Una acción ordinaria de LayerX no requiere una transacción de Paxeer.

Este repositorio es el monorepositorio de Sidiora Labs para LayerX y Paxeer Network. La colocalización mantiene el protocolo, la red de liquidación, los contratos y las superficies de desarrollador auditables en un solo lugar. Cada subsistema conserva su propio límite de compilación, publicación, despliegue y confianza. Véase [`spec/layerx-protocol/design.md`](../../spec/layerx-protocol/design.md).

## Probar la red de prueba

La ruta completa está en [`docs/wiki/Quickstart.md`](../wiki/Quickstart.md): instalar la CLI `layerx` desde `platform/cli`, levantar el clúster, hacer `source` de `build/beta-cluster/env`, luego crear una credencial, reclamar en el faucet, enviar una actividad, verificar el recibo y desplegar un programa.

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

## Compilar desde el código fuente

El runtime principal es C17 (`-std=c17` en el `Makefile` raíz). Los espacios de trabajo agent, human y platform usan Rust 1.91.1 (`rust-toolchain.toml`). Los contratos de liquidación de LayerX usan Solidity 0.8.27 (`foundry.toml`). La cualificación de replay necesita GCC 13, Clang 18, Docker, un ejecutor musl amd64 y un compilador cruzado AArch64 más QEMU; véase [`docs/QUALIFICATION.md`](../QUALIFICATION.md).

```sh
make build
make test
make test-contracts
make ci
```

Objetivos acotados de Paxeer, sin cambiar de directorio:

```sh
make paxeer-build
make paxeer-lint
make paxeer-test
make paxeer-ci
```

`make ci` ejecuta `public-audit`, pruebas nativas, una comparación de archivos de dos compilaciones, comprobaciones de símbolos de consenso y suites de sanitizadores. `make monorepo-ci` es una puerta de verificación transversal entre subsistemas. Un pase local no autoriza a desplegar contratos, mover custodia ni manejar activos reales.

## Estructura del repositorio

| Path | Purpose |
| --- | --- |
| `src/`, `include/` | Runtime de protocolo C17, máquina de estados, almacenamiento, secuenciación, replay e integración de liquidación |
| `cmd/` | Daemons y herramientas nativos (`layerxd`, `layerxctl`, genesis, verify) |
| `agent/` | Interfaz de agente en Rust, SDK, daemon, servidor MCP, codificación, criptografía y verificación de pruebas |
| `human/` | Plano de control humano, compilador de intenciones tipadas, cliente de frontera de custodia, índice del explorador y aplicación web |
| `platform/` | Plataforma de desarrollador, servicios alojados, middleware, SDKs, emulador, CLI y herramientas de publicación |
| `programs/` | Runtime programable de LayerX y herramientas de programas |
| `interop/` | Superficies de comercio entre agentes e interoperabilidad entre redes |
| `contracts/` | Contratos Solidity para custodia de Paxeer, puntos de control, fianzas de garantes, reclamaciones, disputas y salidas |
| `paxeer-network/` | Nodo de Paxeer Network, compatibilidad EVM/RPC, motores de almacenamiento, módulos, contratos y compilaciones locales del subsistema |
| `spec/` | Especificaciones normativas KVX, diseños generados, requisitos y grafos de tareas |
| `tests/`, `test/`, `fuzz/` | Suites nativas, de contratos, replay, invariantes, fallos y fuzz |
| `migrations/` | Trabajo de génesis, migración, conciliación y shadow-replay |
| `docs/` | Wiki, notas del monorepositorio y documentación de cualificación |

## Documentación

- Índice de la wiki: [`docs/wiki/Home.md`](../wiki/Home.md)
- Disposición del monorepositorio y etiquetas de publicación: [`docs/MONOREPO.md`](../MONOREPO.md)
- Puertas de cualificación: [`docs/QUALIFICATION.md`](../QUALIFICATION.md)
- Especificaciones: [`spec/`](../../spec/)

## SDKs e integraciones

| Language | Path |
| --- | --- |
| Rust | `agent/crates/layerx-sdk` |
| Python | `agent/sdk/python` |
| TypeScript | `agent/sdk/typescript` |
| Go | `platform/sdk/go` |
| JVM | `platform/sdk/jvm` |
| .NET | `platform/sdk/dotnet` |
| Swift | `platform/sdk/swift` |

`layerx-agentd` está en `agent/crates/layerx-agentd`. El servidor MCP está en `agent/crates/layerx-mcp`. La CLI de desarrollador en `platform/cli` instala esos transportes con `layerx install mcp` y `layerx install a2a`.

## Cómo contribuir

Lea [`CONTRIBUTING.md`](../../CONTRIBUTING.md) antes de abrir un pull request. Los cambios de protocolo empiezan en `spec/`. No divulgue una vulnerabilidad sospechada en una incidencia pública; siga [`SECURITY.md`](../../SECURITY.md).

## Seguridad

Informe de vulnerabilidades mediante el reporte privado de GitHub, como se describe en [`SECURITY.md`](../../SECURITY.md).

## Licencia

Licenciado bajo la Licencia Apache, versión 2.0. Véase [`LICENSE`](../../LICENSE) y [`NOTICE`](../../NOTICE).

LayerX lo desarrolla Sidiora Labs.
