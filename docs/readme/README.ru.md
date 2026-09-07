<p align="center"><img src="../../layerx-network.png" alt="LayerX" width="720"></p>

<h1 align="center">LayerX</h1>

Детерминированная сеть исполнения и учёта для автономных агентов.

[English](../../README.md) · [Español](README.es.md) · [日本語](README.ja.md) · Русский · [简体中文](README.zh-CN.md) · [Português](README.pt-BR.md) · [Deutsch](README.de.md) · [Français](README.fr.md)

*Если перевод расходится с английской README, эталонной считается английская версия.*

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](../../LICENSE)
[![CI](https://github.com/Sidiora-Labs/LayerX-Network/actions/workflows/ci.yml/badge.svg)](../../.github/workflows/ci.yml)

## Что такое LayerX

LayerX является детерминированной сетью исполнения и учёта для автономных агентов. Каждая операция, изменяющая состояние, поступает как подписанный, канонически закодированный `Activity`. Протокол проверяет субъекта и его полномочия, потребляет последовательность счёта, упорядочивает активность в одной глобальной последовательности, применяет детерминированный переход состояния и возвращает подписанную квитанцию, привязанную к получившемуся корню состояния.

Журнал активностей только для дополнения является источником истины. Индексы базы данных являются отбрасываемыми проекциями и могут быть пересобраны повторным проигрыванием этого журнала. Исполнение, критичное для консенсуса, исключает числа с плавающей точкой, решения по локальным часам, порядок итерации базы данных и другие источники недетерминизма. `402LXP` является единственным компонентом, которому разрешено записывать балансы. Модули протокола выпускают проверенные наборы переводов, а не изменяют средства самостоятельно.

Обычная активность агента исполняется и упорядочивается внутри LayerX. Периодические контрольные точки рассчитываются в Paxeer, который ведёт кастодиальное хранение, регистрацию контрольных точек, гарантийные депозиты, оспаривания, выводы, споры и аварийные выходы. Обычное действие LayerX не требует транзакции Paxeer.

Этот репозиторий является монорепозиторием Sidiora Labs для LayerX и Paxeer Network. Совместное размещение делает протокол, сеть расчётов, контракты и поверхности разработчика проверяемыми в одном месте. Каждая подсистема сохраняет собственную сборку, выпуск, развёртывание и границу доверия. См. [`spec/layerx-protocol/design.md`](../../spec/layerx-protocol/design.md).

## Попробовать тестовую сеть

Полный путь описан в [`docs/wiki/Quickstart.md`](../wiki/Quickstart.md): установите CLI `layerx` из `platform/cli`, поднимите кластер, выполните source `build/beta-cluster/env`, затем создайте учётные данные, запросите средства из faucet, отправьте активность, проверьте квитанцию и разверните программу.

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

## Сборка из исходников

Ядро среды исполнения написано на C17 (`-std=c17` в корневом `Makefile`). Рабочие пространства agent, human и platform используют Rust 1.91.1 (`rust-toolchain.toml`). Контракты расчётов LayerX используют Solidity 0.8.27 (`foundry.toml`). Квалификация повторного проигрывания требует GCC 13, Clang 18, Docker, amd64 musl runner и кросс-компилятор AArch64 плюс QEMU; см. [`docs/QUALIFICATION.md`](../QUALIFICATION.md).

```sh
make build
make test
make test-contracts
make ci
```

Ограниченные цели Paxeer, без смены каталогов:

```sh
make paxeer-build
make paxeer-lint
make paxeer-test
make paxeer-ci
```

`make ci` запускает `public-audit`, нативные тесты, сравнение архивов двух сборок, проверки символов консенсуса и наборы санитайзеров. `make monorepo-ci` является отдельным межподсистемным шлюзом. Локальный проход не является разрешением развёртывать контракты, перемещать кастоди или работать с реальными активами.

## Структура репозитория

| Path | Назначение |
| --- | --- |
| `src/`, `include/` | Среда исполнения протокола на C17, машина состояний, хранилище, упорядочивание, повторное проигрывание и интеграция расчётов |
| `cmd/` | Нативные демоны и инструменты (`layerxd`, `layerxctl`, genesis, verify) |
| `agent/` | Интерфейс агента на Rust, SDK, демон, MCP-сервер, кодирование, криптография и проверка доказательств |
| `human/` | Плоскость управления человеком, компилятор типизированных намерений, клиент кастодиальной границы, индекс обозревателя и веб-приложение |
| `platform/` | Платформа разработчика, хостинг-сервисы, промежуточное ПО, SDK, эмулятор, CLI и инструменты выпуска |
| `programs/` | Программируемая среда исполнения LayerX и инструменты программ |
| `interop/` | Поверхности агентской коммерции и межсетевой совместимости |
| `contracts/` | Контракты Solidity для кастоди Paxeer, контрольных точек, гарантийных депозитов, требований, споров и выходов |
| `paxeer-network/` | Узел Paxeer Network, совместимость EVM/RPC, движки хранения, модули, контракты и локальные сборки подсистемы |
| `spec/` | Нормативные спецификации KVX, сгенерированные проекты, требования и графы задач |
| `tests/`, `test/`, `fuzz/` | Наборы нативных, контрактных, replay, инвариантных, fault и fuzz тестов |
| `migrations/` | Работы по genesis, миграции, сверке и теневому повторному проигрыванию |
| `docs/` | Wiki, заметки по монорепозиторию и документация квалификации |

## Документация

- Индекс wiki: [`docs/wiki/Home.md`](../wiki/Home.md)
- Структура монорепозитория и теги выпуска: [`docs/MONOREPO.md`](../MONOREPO.md)
- Квалификационные шлюзы: [`docs/QUALIFICATION.md`](../QUALIFICATION.md)
- Спецификации: [`spec/`](../../spec/)

## SDK и интеграции

| Язык | Path |
| --- | --- |
| Rust | `agent/crates/layerx-sdk` |
| Python | `agent/sdk/python` |
| TypeScript | `agent/sdk/typescript` |
| Go | `platform/sdk/go` |
| JVM | `platform/sdk/jvm` |
| .NET | `platform/sdk/dotnet` |
| Swift | `platform/sdk/swift` |

`layerx-agentd` находится в `agent/crates/layerx-agentd`. MCP-сервер находится в `agent/crates/layerx-mcp`. CLI разработчика в `platform/cli` устанавливает эти транспорты командами `layerx install mcp` и `layerx install a2a`.

## Участие

Прочитайте [`CONTRIBUTING.md`](../../CONTRIBUTING.md) перед открытием pull request. Изменения протокола начинаются в `spec/`. Не раскрывайте предполагаемую уязвимость в публичном issue; следуйте [`SECURITY.md`](../../SECURITY.md).

## Безопасность

Сообщайте об уязвимостях через приватные отчёты GitHub, как описано в [`SECURITY.md`](../../SECURITY.md).

## Лицензия

Лицензировано на условиях Apache License, Version 2.0. См. [`LICENSE`](../../LICENSE) и [`NOTICE`](../../NOTICE).

LayerX разрабатывается Sidiora Labs.
