<p align="center"><img src="../../layerx-network.png" alt="LayerX" width="720"></p>

<h1 align="center">LayerX</h1>

面向自主智能体的确定性执行与记账网络。

[English](../../README.md) · [Español](README.es.md) · [日本語](README.ja.md) · [Русский](README.ru.md) · 简体中文 · [Português](README.pt-BR.md) · [Deutsch](README.de.md) · [Français](README.fr.md)

*当本文与英文 README 不一致时，以英文 README 为参考版本。*

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](../../LICENSE)
[![CI](https://github.com/Sidiora-Labs/LayerX-Protocol/actions/workflows/ci.yml/badge.svg)](../../.github/workflows/ci.yml)

## LayerX 是什么

LayerX 是面向自主智能体的确定性执行与记账网络。每一次改变状态的操作都以经过签名、按规范编码的 `Activity` 进入系统。协议核验执行者及其权限，消耗账户序号，将活动排入一条全局序列，应用确定性状态转移，并返回与结果状态根绑定的签名回执。

只追加的活动日志才是权威来源。数据库索引是可丢弃的投影，可通过重放该日志重建。共识关键路径上的执行排除浮点运算、本地时钟决策、数据库迭代顺序以及其他非确定性来源。只有 `402LXP` 可以写入余额。协议模块发出经过校验的转账集合，而不是自行改动资金。

普通智能体活动在 LayerX 内部执行并排序。周期性检查点结算到 Paxeer，由其负责托管、检查点登记、担保人保证金、挑战、提现、争议和紧急退出。普通 LayerX 操作不需要一笔 Paxeer 交易。

本仓库是 Sidiora Labs 为 LayerX 与 Paxeer Network 设立的 monorepo。将协议、结算网络、合约和开发者界面放在一起，便于在同一处审计。各子系统保留各自的构建、发布、部署和信任边界。参见 [`spec/layerx-protocol/design.md`](../../spec/layerx-protocol/design.md)。

## 试用测试网

完整路径见 [`docs/wiki/Quickstart.md`](../wiki/Quickstart.md)：从 `platform/cli` 安装 `layerx` CLI，拉起集群，source `build/beta-cluster/env`，然后创建凭证、从水龙头领取、提交活动、核验回执并部署程序。

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

## 从源码构建

核心运行时是 C17（根目录 `Makefile` 中的 `-std=c17`）。agent、human 和 platform 工作区使用 Rust 1.91.1（`rust-toolchain.toml`）。LayerX 结算合约使用 Solidity 0.8.27（`foundry.toml`）。重放验收需要 GCC 13、Clang 18、Docker、amd64 musl 运行器，以及 AArch64 交叉编译器加 QEMU；参见 [`docs/QUALIFICATION.md`](../QUALIFICATION.md)。

```sh
make build
make test
make test-contracts
make ci
```

Paxeer 限定目标，无需切换目录：

```sh
make paxeer-build
make paxeer-lint
make paxeer-test
make paxeer-ci
```

`make ci` 会运行 `public-audit`、原生测试、两次构建的归档比对、共识符号检查以及 sanitizer 套件。`make monorepo-ci` 是独立的跨子系统门禁。本地通过并不等于获准部署合约、转移托管或处理真实资产。

## 仓库布局

| 路径 | 用途 |
| --- | --- |
| `src/`, `include/` | C17 协议运行时、状态机、存储、排序、重放与结算集成 |
| `cmd/` | 原生守护进程与工具（`layerxd`、`layerxctl`、genesis、verify） |
| `agent/` | Rust 智能体接口、SDK、守护进程、MCP 服务器、编码、密码学与证明核验 |
| `human/` | 人工控制面、类型化意图编译器、托管边界客户端、浏览器索引与 Web 应用 |
| `platform/` | 开发者平台、托管服务、中间件、SDK、模拟器、CLI 与发布工具 |
| `programs/` | 可编程 LayerX 运行时与程序工具 |
| `interop/` | 智能体商务与跨网络互操作界面 |
| `contracts/` | 用于 Paxeer 托管、检查点、担保人保证金、索赔、争议和退出的 Solidity 合约 |
| `paxeer-network/` | Paxeer Network 节点、EVM/RPC 兼容、存储引擎、模块、合约及子系统本地构建 |
| `spec/` | 规范性 KVX 规格、生成的设计、需求与任务图 |
| `tests/`, `test/`, `fuzz/` | 原生、合约、重放、不变量、故障与模糊测试套件 |
| `migrations/` | 创世、迁移、对账与影子重放工作 |
| `docs/` | Wiki、monorepo 说明与验收文档 |

## 文档

- Wiki 索引：[`docs/wiki/Home.md`](../wiki/Home.md)
- Monorepo 布局与发布标签：[`docs/MONOREPO.md`](../MONOREPO.md)
- 验收门禁：[`docs/QUALIFICATION.md`](../QUALIFICATION.md)
- 规格：[`spec/`](../../spec/)

## SDK 与集成

| 语言 | 路径 |
| --- | --- |
| Rust | `agent/crates/layerx-sdk` |
| Python | `agent/sdk/python` |
| TypeScript | `agent/sdk/typescript` |
| Go | `platform/sdk/go` |
| JVM | `platform/sdk/jvm` |
| .NET | `platform/sdk/dotnet` |
| Swift | `platform/sdk/swift` |

`layerx-agentd` 位于 `agent/crates/layerx-agentd`。MCP 服务器位于 `agent/crates/layerx-mcp`。`platform/cli` 中的开发者 CLI 通过 `layerx install mcp` 和 `layerx install a2a` 安装这些传输。

## 贡献

提交 pull request 前请阅读 [`CONTRIBUTING.md`](../../CONTRIBUTING.md)。协议变更从 `spec/` 开始。不要在公开 issue 中披露疑似漏洞；请遵循 [`SECURITY.md`](../../SECURITY.md)。

## 安全

通过 GitHub 私下报告渠道提交漏洞，具体见 [`SECURITY.md`](../../SECURITY.md)。

## 许可证

依 Apache 许可证 2.0 版授权。参见 [`LICENSE`](../../LICENSE) 和 [`NOTICE`](../../NOTICE)。

LayerX 由 Sidiora Labs 开发。
