<p align="center"><img src="../../layerx-network.png" alt="LayerX" width="720"></p>

<h1 align="center">LayerX</h1>

自律エージェント向けの決定的実行および会計ネットワーク。

[English](../../README.md) · [Español](README.es.md) · 日本語 · [Русский](README.ru.md) · [简体中文](README.zh-CN.md) · [Português](README.pt-BR.md) · [Deutsch](README.de.md) · [Français](README.fr.md)

*内容が異なる場合は、英語版 README を参照版とします。*

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](../../LICENSE)
[![CI](https://github.com/Sidiora-Labs/LayerX-Network/actions/workflows/ci.yml/badge.svg)](../../.github/workflows/ci.yml)

## LayerX とは

LayerX は自律エージェント向けの決定的実行および会計ネットワークです。状態を変更する操作はすべて、署名済みで正規符号化された `Activity` として投入されます。プロトコルは行為者と権限を検証し、アカウントシーケンスを消費し、単一のグローバルシーケンス上で Activity を順序付け、決定的な状態遷移を適用し、結果の状態ルートに紐付く署名済みレシートを返します。

追記専用の Activity ログが権威です。データベース索引は破棄可能な射影であり、そのログをリプレイすれば再構築できます。コンセンサスに関わる実行は浮動小数点、ローカル時計による判断、データベースの反復順、その他の非決定性の源を排除します。残高の書き込みが許されるコンポーネントは `402LXP` のみです。プロトコルモジュールは資金を自ら変更せず、検証済みの転送集合を発行します。

通常のエージェント Activity は LayerX 内で実行および順序付けされます。定期チェックポイントは Paxeer に決済され、Paxeer はカストディ、チェックポイント登録、保証人ボンド、チャレンジ、出金、紛争、緊急退出を保持します。通常の LayerX 操作に Paxeer トランザクションは不要です。

本リポジトリは LayerX および Paxeer Network 向けの Sidiora Labs モノレポです。同一配置により、プロトコル、決済ネットワーク、コントラクト、開発者向け面を一箇所で監査できます。各サブシステムは独自のビルド、リリース、デプロイ、信頼境界を維持します。[`spec/layerx-protocol/design.md`](../../spec/layerx-protocol/design.md) を参照してください。

## テストネットを試す

全手順は [`docs/wiki/Quickstart.md`](../wiki/Quickstart.md) です。`platform/cli` から `layerx` CLI をインストールし、クラスタを起動し、`build/beta-cluster/env` を source したうえで、クレデンシャルを作成し、faucet から請求し、Activity を送信し、レシートを検証し、プログラムをデプロイします。

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

## ソースからビルドする

コアランタイムは C17 です（ルート `Makefile` の `-std=c17`）。agent、human、platform の各ワークスペースは Rust 1.91.1（`rust-toolchain.toml`）を使用します。LayerX 決済コントラクトは Solidity 0.8.27（`foundry.toml`）を使用します。リプレイ適格化には GCC 13、Clang 18、Docker、amd64 musl ランナー、AArch64 クロスコンパイラと QEMU が必要です。[`docs/QUALIFICATION.md`](../QUALIFICATION.md) を参照してください。

```sh
make build
make test
make test-contracts
make ci
```

ディレクトリを変更せずに、Paxeer の範囲付きターゲット:

```sh
make paxeer-build
make paxeer-lint
make paxeer-test
make paxeer-ci
```

`make ci` は `public-audit`、ネイティブテスト、2 回ビルドのアーカイブ比較、コンセンサスシンボル検査、サニタイザスイートを実行します。`make monorepo-ci` は別のクロスサブシステムゲートです。ローカルでの合格は、コントラクトのデプロイ、カストディの移動、実資産の取り扱いを許可するものではありません。

## リポジトリ構成

| Path | 用途 |
| --- | --- |
| `src/`, `include/` | C17 プロトコルランタイム、ステートマシン、ストレージ、シーケンシング、リプレイ、決済統合 |
| `cmd/` | ネイティブデーモンとツール（`layerxd`、`layerxctl`、genesis、verify） |
| `agent/` | Rust エージェントインタフェース、SDK、デーモン、MCP サーバ、エンコーディング、暗号、証明検証 |
| `human/` | 人間向けコントロールプレーン、型付きインテントコンパイラ、カストディ境界クライアント、エクスプローラ索引、Web アプリケーション |
| `platform/` | 開発者プラットフォーム、ホスト型サービス、ミドルウェア、SDK、エミュレータ、CLI、リリーストール |
| `programs/` | プログラマブル LayerX ランタイムおよびプログラムツール |
| `interop/` | エージェントコマースおよびクロスネットワーク相互運用面 |
| `contracts/` | Paxeer のカストディ、チェックポイント、保証人ボンディング、請求、紛争、退出向け Solidity コントラクト |
| `paxeer-network/` | Paxeer Network ノード、EVM/RPC 互換、ストレージエンジン、モジュール、コントラクト、サブシステムローカルビルド |
| `spec/` | 規範的 KVX 仕様、生成された設計、要件、タスクグラフ |
| `tests/`, `test/`, `fuzz/` | ネイティブ、コントラクト、リプレイ、不変条件、フォルト、ファズの各スイート |
| `migrations/` | ジェネシス、マイグレーション、突合、シャドウリプレイの作業 |
| `docs/` | Wiki、モノレポ注記、適格化ドキュメント |

## ドキュメント

- Wiki 索引: [`docs/wiki/Home.md`](../wiki/Home.md)
- モノレポ構成とリリースタグ: [`docs/MONOREPO.md`](../MONOREPO.md)
- 適格化ゲート: [`docs/QUALIFICATION.md`](../QUALIFICATION.md)
- 仕様: [`spec/`](../../spec/)

## SDK と統合

| 言語 | Path |
| --- | --- |
| Rust | `agent/crates/layerx-sdk` |
| Python | `agent/sdk/python` |
| TypeScript | `agent/sdk/typescript` |
| Go | `platform/sdk/go` |
| JVM | `platform/sdk/jvm` |
| .NET | `platform/sdk/dotnet` |
| Swift | `platform/sdk/swift` |

`layerx-agentd` は `agent/crates/layerx-agentd` です。MCP サーバは `agent/crates/layerx-mcp` です。`platform/cli` の開発者 CLI は `layerx install mcp` および `layerx install a2a` でこれらのトランスポートをインストールします。

## コントリビューション

プルリクエストを開く前に [`CONTRIBUTING.md`](../../CONTRIBUTING.md) を読んでください。プロトコル変更は `spec/` から始めます。疑わしい脆弱性を公開 Issue に記載しないでください。[`SECURITY.md`](../../SECURITY.md) に従ってください。

## セキュリティ

脆弱性は GitHub の非公開報告機能で報告してください。手順は [`SECURITY.md`](../../SECURITY.md) にあります。

## ライセンス

Apache License, Version 2.0 のもとでライセンスされます。[`LICENSE`](../../LICENSE) および [`NOTICE`](../../NOTICE) を参照してください。

LayerX は Sidiora Labs が開発しています。
