<!-- id: beta_contract -->
<!-- readiness_claim: false -->

# LayerX beta contract

This is the canonical LayerX beta contract. It is the only statement of the surfaces and journeys the beta supports, the beta endpoints and hostnames, the network id, the wire protocol version, the beta CA, the artifact set, the evidence rung each surface must reach and has reached, the unknown-state behaviour, the external dependencies with their beta counterparts and the beta-versus-production differences. `tools/ci/beta-contract-check.sh` checks the install docs, the hosted manifests, the release manifest, the release workflow, the hosted status surface and the docs content index against this document; any disagreement fails the build.

**This beta is not ready.** The readiness claim below is `false` and stays `false` until every surface has reached its required rung through an executed gate recorded in `spec/layerx-beta/qualification.kvx` and every contradiction listed here has been resolved in its source.

## Identity

| Key | Value |
| --- | --- |
| id | beta_contract |
| readiness_claim | false |
| readiness_statement | The LayerX beta is NOT ready: no gate record exists, every surface is at rung source_present, and the cross-source contradictions listed in this contract are open. |
| beta_domain | layerx.network |
| required_rung_functional | runtime_proven |
| required_rung_hosted | deployment_proven |
| rung_order | source_present < statically_coherent < built < tested < runtime_proven < deployment_proven < owner_certified |
| evidence_ledger | spec/layerx-beta/qualification.kvx |
| contract_check | tools/ci/beta-contract-check.sh |
| ledger_check | tools/ci/beta-ledger-check.sh |

The reached rung of a surface is raised only by a `[gate.*]` record in the evidence ledger. No such record exists at the revision this contract describes, so every reached rung below is `source_present`.

## Surfaces and journeys

| Surface | Journey | Class | Required rung | Reached rung | Source |
| --- | --- | --- | --- | --- | --- |
| native-core | ledger transition, checkpoints, settlement, guarantor bonds | functional | runtime_proven | source_present | src/, include/ |
| native-daemon | layerxd, layerxctl, layerx-verify, layerx-genesis | functional | runtime_proven | source_present | cmd/ |
| settlement-contracts | CheckpointRegistry, LayerXCustody, WithdrawalClaims, EmergencyExit, GuarantorBond | functional | runtime_proven | source_present | contracts/ |
| mirror-ethereum | LayerXMirrorArchive on ethereum-sepolia, base-sepolia and hood-testnet | hosted | deployment_proven | source_present | interop/contracts/ethereum-mirror, interop/deploy/mirror/ethereum-testnets-2026-08-31.json |
| mirror-solana | layerx-solana-mirror-program on solana-devnet | hosted | deployment_proven | source_present | interop/contracts/solana-mirror, interop/deploy/mirror/solana-devnet-2026-08-31.json |
| agent-daemon | layerx-agentd admission, signing and receipts | functional | runtime_proven | source_present | agent/crates/layerx-agentd |
| agent-mcp | MCP tool surface for agents | functional | runtime_proven | source_present | agent/crates/layerx-mcp |
| sdk-typescript | @sidiora/layerx-sdk | functional | runtime_proven | source_present | agent/sdk/typescript |
| sdk-python | layerx-sdk on PyPI | functional | runtime_proven | source_present | agent/sdk/python |
| sdk-rust | layerx-sdk on crates.io | functional | runtime_proven | source_present | agent/crates/layerx-sdk |
| sdk-go | github.com/Sidiora-Labs/LayerX-Protocol/platform/sdk/go | functional | runtime_proven | source_present | platform/sdk/go |
| sdk-jvm | com.sidiora.layerx:layerx-sdk | functional | runtime_proven | source_present | platform/sdk/jvm |
| sdk-swift | LayerXSDK | functional | runtime_proven | source_present | platform/sdk/swift |
| sdk-dotnet | LayerX.Sdk | functional | runtime_proven | source_present | platform/sdk/dotnet |
| human-service | custody, intents, approvals, explorer index | functional | runtime_proven | source_present | human/crates/layerx-human-service, human/crates/layerx-intents, human/crates/layerx-explorer-index |
| human-web | onboarding | functional | runtime_proven | source_present | human/apps/web/e2e/onboarding.spec.ts |
| human-web | wallet-binding | functional | runtime_proven | source_present | human/apps/web/e2e/custody.spec.ts |
| human-web | deposit | functional | runtime_proven | source_present | human/apps/web/e2e/custody.spec.ts |
| human-web | move-money | functional | runtime_proven | source_present | human/apps/web/e2e/move.spec.ts |
| human-web | create-agent | functional | runtime_proven | source_present | human/apps/web/e2e/agents.spec.ts |
| human-web | approval-grant | functional | runtime_proven | source_present | human/apps/web/e2e/approvals.spec.ts |
| human-web | approval-reject | functional | runtime_proven | source_present | human/apps/web/e2e/approvals.spec.ts |
| human-web | withdrawal-claim | functional | runtime_proven | source_present | human/apps/web/e2e/custody.spec.ts, contracts/WithdrawalClaims.sol |
| human-web | emergency-exit | functional | runtime_proven | source_present | human/apps/web/e2e/custody.spec.ts, contracts/EmergencyExit.sol |
| human-web | explorer, activity, settings, support | functional | runtime_proven | source_present | human/apps/web/e2e/explorer.spec.ts, activity.spec.ts, settings.spec.ts, support.spec.ts |
| platform-cli | layerx CLI: environments, keys, programs, install | functional | runtime_proven | source_present | platform/cli |
| emulator | local emulator with clock, prefund, fault and snapshot control | functional | runtime_proven | source_present | platform/emulator |
| hosted-testnet | testnet-control status, parameters, funding and reset | hosted | deployment_proven | source_present | platform/hosted/testnet |
| hosted-faucet | bearer-authenticated faucet claims with durable quotas | hosted | deployment_proven | source_present | platform/hosted/faucet |
| hosted-gateway | developer gateway: payments, receipts, typed refusals | hosted | deployment_proven | source_present | platform/hosted/gateway |
| hosted-registry | hosted program registry | hosted | deployment_proven | source_present | platform/hosted/registry |
| hosted-webhooks | signed webhook deliveries | hosted | deployment_proven | source_present | platform/hosted/webhooks |
| hosted-dashboard | developer dashboard API and web | hosted | deployment_proven | source_present | platform/hosted/dashboard |
| hosted-node | layerxd StatefulSet with bootstrap, supervisor and settlement binding; hosts the core, receipt-authority and agent-boundary containers | hosted | deployment_proven | source_present | platform/hosted/node |
| hosted-core | core boundary: layerx-pending-core and layerx-pending-core-admin over the node LNI socket | hosted | deployment_proven | source_present | platform/hosted/core |
| hosted-authority | independent receipt authority replica: layerx-receipt-authority | hosted | deployment_proven | source_present | platform/hosted/authority |
| hosted-agent-boundary | agent boundary: layerx-agent-boundary LNI submissions for the gateway and the registry | hosted | deployment_proven | source_present | platform/hosted/agent-boundary |
| hosted-identity | identity service: principals, sessions, introspection and service tokens | hosted | deployment_proven | source_present | platform/hosted/identity |
| hosted-paxeer | Paxeer chain 125 node with its JSON-RPC boundary, chain initialisation and settlement contract deployment | hosted | deployment_proven | source_present | platform/hosted/paxeer |
| hosted-internal | layerx-internal aliases, Redis, KMS and event sources for the developer plane | hosted | deployment_proven | source_present | platform/hosted/internal |
| hosted-tests | static topology check: every configured in-cluster URL resolves to an exposed Service port admitted by NetworkPolicy on both ends | hosted | deployment_proven | source_present | platform/hosted/tests |
| middleware-buyer | @sidiora/layerx-buyer-middleware | functional | runtime_proven | source_present | platform/middleware/buyer |
| middleware-seller | @sidiora/layerx-seller-middleware | functional | runtime_proven | source_present | platform/middleware/seller |
| middleware-merchant | @sidiora/layerx-merchant-middleware | functional | runtime_proven | source_present | platform/middleware/merchant |
| middleware-agent | @sidiora/layerx-agent-middleware | functional | runtime_proven | source_present | platform/middleware/agent |
| docs-site | documentation site and its executable samples | functional | runtime_proven | source_present | platform/docs |
| reference-app-buyer-agent | @sidiora/layerx-example-buyer-agent | functional | runtime_proven | source_present | platform/examples/buyer-agent |
| reference-app-paid-api | @sidiora/layerx-example-paid-api | functional | runtime_proven | source_present | platform/examples/paid-api |
| reference-app-merchant-shop | @sidiora/layerx-example-merchant-shop | functional | runtime_proven | source_present | platform/examples/merchant-shop |
| reference-app-marketplace | @sidiora/layerx-example-marketplace | functional | runtime_proven | source_present | platform/examples/marketplace |
| integration-express | Express framework integration | functional | runtime_proven | source_present | platform/integrations/express |
| integration-next | Next.js framework integration | functional | runtime_proven | source_present | platform/integrations/next |
| integration-fastapi | FastAPI framework integration | functional | runtime_proven | source_present | platform/integrations/fastapi |
| integration-spring | Spring framework integration | functional | runtime_proven | source_present | platform/integrations/spring |
| integration-ios | iOS application integration | functional | runtime_proven | source_present | platform/integrations/ios |
| integration-android | Android application integration | functional | runtime_proven | source_present | platform/integrations/android |
| integration-agents | @sidiora/layerx-agent-integrations package | functional | runtime_proven | source_present | platform/integrations/agents |
| agent-framework-mcp | MCP server journey | functional | runtime_proven | source_present | platform/integrations/agents/src/mcp.ts |
| agent-framework-a2a | A2A journey | functional | runtime_proven | source_present | platform/integrations/agents/src/a2a.ts |
| agent-framework-openai | OpenAI tools journey | functional | runtime_proven | source_present | platform/integrations/agents/src/openai.ts |
| agent-framework-anthropic | Anthropic tools journey | functional | runtime_proven | source_present | platform/integrations/agents/src/anthropic.ts |
| agent-framework-langchain | LangChain tools journey | functional | runtime_proven | source_present | platform/integrations/agents/src/langchain.ts |
| agent-framework-vercel-ai | Vercel AI tools journey | functional | runtime_proven | source_present | platform/integrations/agents/src/vercel-ai.ts |
| programs-runtime | deploy | functional | runtime_proven | source_present | programs/crates/layerx-programs-runtime |
| programs-runtime | paid-call | functional | runtime_proven | source_present | programs/crates/layerx-programs-runtime |
| programs-runtime | restart | functional | runtime_proven | source_present | programs/crates/layerx-programs-runtime |
| programs-interpreter | program interpreter | functional | runtime_proven | source_present | programs/crates/layerx-programs-interpreter |
| programs-market | program market | functional | runtime_proven | source_present | programs/crates/layerx-programs-market |
| programs-protocol-adapter | protocol adapter | functional | runtime_proven | source_present | programs/crates/layerx-programs-protocol-adapter |
| programs-registry | program registry library | functional | runtime_proven | source_present | programs/crates/layerx-programs-registry |
| programs-sandbox | program sandbox | functional | runtime_proven | source_present | programs/crates/layerx-programs-sandbox |
| interop-x402 | x402 payments | functional | runtime_proven | source_present | interop/crates/layerx-x402 |
| interop-ap2 | AP2 mandates | functional | runtime_proven | source_present | interop/crates/layerx-ap2 |
| interop-ucp | UCP | functional | runtime_proven | source_present | interop/crates/layerx-ucp |
| interop-visa-tap | Visa trusted agent protocol | functional | runtime_proven | source_present | interop/crates/layerx-visa-tap |
| interop-portable | portable verification | functional | runtime_proven | source_present | interop/crates/layerx-portable |
| interop-migrate | migration | functional | runtime_proven | source_present | interop/crates/layerx-migrate |
| interop-fiat | fiat | functional | runtime_proven | source_present | interop/crates/layerx-fiat |
| interop-mirror | mirror archives | functional | runtime_proven | source_present | interop/crates/layerx-mirror |
| interop-gateway | interop gateway | functional | runtime_proven | source_present | interop/crates/layerx-interop-gateway |
| interop-service | interop service | functional | runtime_proven | source_present | interop/crates/layerx-interop-service |
| ramps-toolkit | market-maker ramp toolkit | functional | runtime_proven | source_present | platform/ramps/toolkit |
| reference-ramp | reference ramp service | hosted | deployment_proven | source_present | platform/ramps/deployment.yaml |
| multichain-paxeer-boundary | Paxeer custody and guaranteed-withdrawal boundary | hosted | deployment_proven | source_present | human/crates/layerx-paxeer-client, paxeer-network/ |
| multichain-one-ledger | one ledger across mirrors | functional | runtime_proven | source_present | interop/crates/layerx-mirror |

## Beta endpoints and hostnames

| Key | Value | Source |
| --- | --- | --- |
| testnet_public_url | https://testnet.layerx.network | platform/hosted/testnet/deployment.yaml Ingress layerx-testnet-public; .github/workflows/platform.yml LAYERX_TESTNET_URL; platform/hosted/testnet/src/lib.rs; platform/hosted/testnet/status.json; platform/docs/testnet.md |
| gateway_url | https://api.testnet.layerx.network | .github/workflows/platform.yml LAYERX_GATEWAY_URL; platform/hosted/testnet/src/lib.rs; platform/docs/testnet.md; platform/examples/*/layerx.example.json |
| faucet_url | https://faucet.testnet.layerx.network | .github/workflows/platform.yml LAYERX_FAUCET_URL; platform/hosted/testnet/src/lib.rs; platform/docs/testnet.md |
| status_url | https://status.layerx.network | platform/hosted/testnet/src/lib.rs; platform/docs/testnet.md |
| developer_host | developers.layerx.example | platform/hosted/webhooks/deployment.yaml Ingress layerx-developer and layerx-developer-web (placeholder, see Contradictions) |
| ramp_host | ramp.testnet.layerx.network | platform/ramps/deployment.yaml Ingress |
| emulator_endpoint | http://127.0.0.1:9402 | platform/docs/content/install.md; platform/docs/content/environments/emulator.md |
| testnet_core_url | https://layerx-pending-core.layerx-testnet.svc.cluster.local:9443 | platform/hosted/testnet/deployment.yaml LAYERX_TESTNET_CORE_URL; platform/hosted/node/deployment.yaml Service layerx-pending-core |
| testnet_core_admin_url | https://layerx-pending-core-admin.layerx-testnet.svc.cluster.local:9444 | platform/hosted/testnet/deployment.yaml LAYERX_TESTNET_CORE_ADMIN_URL; platform/hosted/node/deployment.yaml Service layerx-pending-core-admin |
| testnet_gateway_url | https://layerx-gateway.layerx-testnet.svc.cluster.local:443 | platform/hosted/testnet/deployment.yaml LAYERX_TESTNET_GATEWAY_URL |
| testnet_paxeer_url | https://paxeer-boundary.layerx-testnet.svc.cluster.local:9443 | platform/hosted/testnet/deployment.yaml LAYERX_TESTNET_PAXEER_URL; platform/hosted/paxeer/deployment.yaml Service paxeer-boundary |
| gateway_component_url | https://layerx-agent-boundary.layerx-testnet.svc.cluster.local:9443 | platform/hosted/gateway/deployment.yaml LAYERX_GATEWAY_COMPONENT_URL; platform/hosted/node/deployment.yaml Service layerx-agent-boundary |
| gateway_authority_url | https://layerx-receipt-authority.layerx-testnet.svc.cluster.local:9443 | platform/hosted/gateway/deployment.yaml LAYERX_GATEWAY_AUTHORITY_URL; platform/hosted/node/deployment.yaml Service layerx-receipt-authority |
| gateway_identity_url | https://layerx-identity.layerx-testnet.svc.cluster.local:9443 | platform/hosted/gateway/deployment.yaml LAYERX_GATEWAY_IDENTITY_URL; platform/hosted/identity/deployment.yaml Service layerx-identity |
| gateway_program_registry_url | https://layerx-program-registry.layerx-testnet.svc.cluster.local:9420 | platform/hosted/gateway/deployment.yaml LAYERX_GATEWAY_PROGRAM_REGISTRY_URL |

## Network id

| Key | Value | Source |
| --- | --- | --- |
| network_id | 402 | platform/docs/content/install.md `--network-id 402`; platform/hosted/testnet/src/lib.rs TESTNET_NETWORK_ID; platform/docs/testnet.md |
| gateway_network_id | layerx-testnet | platform/hosted/gateway/deployment.yaml LAYERX_GATEWAY_NETWORK_ID |

## Wire protocol version

| Key | Value | Source |
| --- | --- | --- |
| wire_protocol_version | 2 | platform/hosted/testnet/deployment.yaml ConfigMap layerx-testnet-release lxp-wire-protocol-version; platform/hosted/gateway/deployment.yaml LAYERX_GATEWAY_LXP_WIRE_VERSION; platform/hosted/webhooks/deployment.yaml LAYERX_WEBHOOKS_LXP_WIRE_VERSION; agent/crates/layerx-wire/src/limits.rs PROTOCOL_VERSION |
| package_semver | 0.1.0 | platform/hosted/testnet/deployment.yaml ConfigMap layerx-testnet-release package-semver; platform/docs/content/install.md JVM coordinate |

## Beta CA

| Key | Value | Source |
| --- | --- | --- |
| internal_ca_secret | layerx-internal-ca | platform/hosted/gateway/deployment.yaml volume trust |
| testnet_control_tls_secret | layerx-testnet-control-tls | platform/hosted/testnet/deployment.yaml (ca.crt mounted at /run/layerx/tls/ca.crt) |
| testnet_ingress_tls_secret | layerx-testnet-ingress-tls | platform/hosted/testnet/deployment.yaml Ingress layerx-testnet-public |
| gateway_ingress_tls_secret | layerx-gateway-ingress-tls | platform/hosted/gateway/deployment.yaml Ingress layerx-gateway |
| ca_file_env | LAYERX_TEST_CA_FILE | .github/workflows/platform.yml hosted-testnet-journey |
| ca_ci_secret | LAYERX_SCHEDULED_TESTNET_CA_BASE64 | .github/workflows/platform.yml hosted-testnet-journey |
| ca_material | owner-supplied under the names above; never in the repository | spec/layerx-beta/spec.kvx decision.beta_infra |

## Artifact set

| Ecosystem | Registry | Surface | Packages | Publication job |
| --- | --- | --- | --- | --- |
| crates-io | https://crates.io | sdk-rust | layerx-agent-api, layerx-client, layerx-crypto, layerx-mirror, layerx-programs-runtime, layerx-proof, layerx-sdk, layerx-types, layerx-wire | present |
| npm | https://registry.npmjs.org | sdk-typescript | @sidiora/layerx-agent-integrations, @sidiora/layerx-agent-middleware, @sidiora/layerx-buyer-middleware, @sidiora/layerx-express, @sidiora/layerx-merchant-middleware, @sidiora/layerx-next, @sidiora/layerx-sdk, @sidiora/layerx-seller-middleware | present |
| pypi | https://pypi.org | sdk-python | layerx-fastapi, layerx-sdk | present |
| go-modules | https://proxy.golang.org | sdk-go | github.com/Sidiora-Labs/LayerX-Protocol/platform/sdk/go | present |
| maven-central | https://repo1.maven.org/maven2 | sdk-jvm | com.sidiora.layerx:layerx-android, com.sidiora.layerx:layerx-sdk, com.sidiora.layerx:layerx-spring-boot-starter | present |
| swiftpm | https://github.com | sdk-swift | LayerXSDK | present |
| nuget | https://www.nuget.org | sdk-dotnet | LayerX.Sdk | present |

| Key | Value |
| --- | --- |
| artifact_manifest_path | platform/release/artifact-manifest.json |
| artifact_manifest_status | not_emitted |
| artifact_manifest_emitter | layerx-platform-release -- manifest |
| artifact_manifest_verifier | layerx-platform-release -- verify |
| artifact_manifest_verification_job | release-verification |
| artifact_manifest_workflow_artifact | release-artifact-manifest |
| release_tag_format | sdk-v{version} |
| source_digest | git-archive-sha256 |

The artifact set of the beta is exactly the content of the artifact manifest at `platform/release/artifact-manifest.json` (schema `layerx/artifact-manifest/1`). The release workflow publishes every ecosystem above through its `publish-<ecosystem>` job only after the release gates pass on the release revision, under the beta pre-release version the tagged revision declares, with an immutable digest, a signature, an SBOM, a provenance attestation and an install check from the registry per artifact, and each publication job retains an artifact record (`artifacts.kvx`) next to the bytes it published. The `release-verification` job then emits the manifest with `layerx-platform-release -- manifest` from the retained publication bundles, listing for every published artifact its name, version, registry, immutable digest (and whether it hashes the built bytes, the bytes the registry serves or the published source archive), signature, SBOM and attestation references, registry location, source revision and rollback identity (the most recent earlier `sdk-v` tag, `null` for the first release); on a tagged release the same job runs `layerx-platform-release -- verify`, which fetches every artifact from its registry, compares its bytes with the manifest digest and halts the release before `release-promotion` naming every failing artifact, and `release-promotion` needs `release-verification` and refuses to promote without the retained manifest. The same verification runs locally against a directory of downloaded artifacts laid out as `<registry>/<artifact file>` with `layerx-platform-release verify --manifest <file> --from <dir>`. The manifest is retained as the workflow artifact `release-artifact-manifest` of every release run and is committed to `platform/release/artifact-manifest.json` with the gate records of the first tagged beta release; `artifact_manifest_status` is `not_emitted` exactly while that file is absent. While the file is absent and this contract states `not_emitted`, the docs may name only package identities declared in `platform/release/registries.kvx` (a Maven install coordinate `group:artifact:version` is compared by its `group:artifact` identity and its version is checked against `package_semver`), and no artifact is part of the beta artifact set; once the file is present, the docs may name only artifacts the manifest lists, an install coordinate that carries a version naming the manifest version; a file absent while this contract does not state `not_emitted` makes every artifact the docs name unlisted. `make platform-release-check` refuses any ecosystem declared without a publication job, any publication job without a declaration, a release workflow without the `release-verification` job or a promotion that does not need it.

### Install coordinates

| Language | Coordinate | Ecosystem |
| --- | --- | --- |
| TypeScript | @sidiora/layerx-sdk | npm |
| Python | layerx-sdk | pypi |
| Go | github.com/Sidiora-Labs/LayerX-Protocol/platform/sdk/go | go-modules |
| Java and Kotlin | com.sidiora.layerx:layerx-sdk:0.1.0 | maven-central |
| Swift | LayerXSDK | swiftpm |
| C# | LayerX.Sdk | nuget |
| Rust | layerx-sdk | crates-io |

## Documentation journeys

| Page | Surface | Journey |
| --- | --- | --- |
| index | docs-site | orientation |
| install | docs-site | install and environment selection |
| concepts/money | native-core | money model |
| concepts/paying | hosted-gateway | paying |
| concepts/receipts | hosted-gateway | receipts |
| concepts/agents | agent-daemon | agents |
| concepts/idempotency | hosted-gateway | idempotency |
| concepts/enforcement | native-core | enforcement |
| quickstart/typescript | sdk-typescript | quickstart |
| quickstart/python | sdk-python | quickstart |
| quickstart/go | sdk-go | quickstart |
| quickstart/jvm | sdk-jvm | quickstart |
| quickstart/swift | sdk-swift | quickstart |
| quickstart/csharp | sdk-dotnet | quickstart |
| quickstart/rust | sdk-rust | quickstart |
| framework/express | integration-express | framework quickstart |
| framework/next | integration-next | framework quickstart |
| framework/fastapi | integration-fastapi | framework quickstart |
| framework/spring | integration-spring | framework quickstart |
| framework/ios | integration-ios | framework quickstart |
| framework/android | integration-android | framework quickstart |
| guide/seller-middleware | middleware-seller | guide |
| guide/buyer-middleware | middleware-buyer | guide |
| guide/merchant-middleware | middleware-merchant | guide |
| guide/agent-middleware | middleware-agent | guide |
| guide/webhooks | hosted-webhooks | guide |
| guide/receipts | hosted-gateway | guide |
| guide/programs | programs-runtime | guide |
| guide/interop | interop-gateway | guide |
| guide/reference-applications | reference-app-buyer-agent, reference-app-paid-api, reference-app-merchant-shop, reference-app-marketplace | guide |
| reference/human-api | human-service | generated reference |
| reference/agent-api | agent-daemon | generated reference |
| reference/errors | hosted-gateway | generated reference |
| reference/enforcement | native-core | generated reference |
| reference/samples | docs-site | generated reference |
| environments/testnet | hosted-testnet | hosted testnet |
| environments/emulator | emulator | emulator |

## Unknown-state behaviour

An outcome that is not known is reported as unknown, never as success, at every layer of the beta:

- A submission whose result the client did not observe stays `unknown` (ramp status vocabulary) or `still_checking` (faucet claims) under its idempotency key and is resolved only by looking up the canonical receipt or activity; it is never resubmitted blindly and never translated into a safe outcome.
- `testnet-control` reports `/readyz` per dependency (identity, faucet, core, core admin, receipt authority, registry, Redis, gateway, Paxeer boundary) and per journey (funding, payment, receipt inspection, Programs), each journey being the conjunction of its declared dependency set; the global state is `ready` only when every dependency is reachable, every journey is ready and the package semantic version and wire protocol version match the pending release, and `/v1/journeys/<journey>` admits a tester only while that journey is ready, naming the failing dependency otherwise. A gateway outage does not imply a core outage and Paxeer degradation is never presented as LayerX finality.
- A gate that cannot run because an owner input is missing is recorded in the evidence ledger with outcome `blocked` and the input named; it is never recorded as `pass`.
- A surface without an executed gate remains at rung `source_present`; a task status never raises a surface above `tested`.
- The readiness claim of this contract stays `false` while any surface is below its required rung or any contradiction listed below is open.

## Architecture summary

The native C17 core (`src/`, `include/`) implements the ledger transition, checkpoints and settlement rules and is operated through the daemons and tools in `cmd/`. Checkpoints are anchored in the Solidity settlement contracts (`contracts/`) and mirrored to Ethereum test networks and Solana devnet by the mirror contracts and the `layerx-mirror` crate (`interop/`). The agent plane (`agent/`) exposes admission, signing and receipts to agents through `layerx-agentd`, the MCP surface and seven SDK ecosystems. The human plane (`human/`) provides the human service, the Paxeer custody client and the web application with its custody, approval, withdrawal-claim and emergency-exit journeys. The developer platform (`platform/`) provides the CLI, the emulator, the hosted testnet, faucet, gateway, program registry, webhooks and dashboard, the four middleware packages, the framework and agent-framework integrations, the documentation site and the four reference applications. The Programs runtime (`programs/`) runs deployed programs behind the hosted registry with deploy, paid-call and restart journeys. Interop (`interop/crates`) covers x402, AP2, UCP, the Visa trusted agent protocol, portable verification, migration and fiat. The ramp toolkit and reference ramp (`platform/ramps`) operate as an independent market maker over ordinary agent accounts, and Paxeer (`paxeer-network/`, `human/crates/layerx-paxeer-client`) remains the sole custody and guaranteed-withdrawal boundary across every mirror.

Checkpoint identity and freshness are declared once, in `contracts/config/checkpoint-settlement.json` (schema `layerx/checkpoint-settlement/1`): the version 2 `checkpoint-certificate` and `guarantor-attestation` domain tags, the canonical header encoding prefix and length, the finality policy (maximum header-relative attestation delay and certificate threshold) and the named settlement domains (Paxeer chain id, settlement contract and bonded guarantor set). The native core reads it through the generated `lxp_checkpoint_settlement.h`, `layerx-proof` embeds it as `layerx_proof::settlement`, the Foundry registry tests construct `CheckpointRegistry` from it, and the cross-language vectors under `tests/vectors/checkpoint/` are generated from it. The `vectors` domain is the only domain declared today; the beta Paxeer testnet domain is added to the same file when the beta cluster bring-up of task 3.7 records the deployed `GuarantorBond` address and chain id.

## External dependencies

| Dependency | Production counterpart | Beta counterpart | Owner input names |
| --- | --- | --- | --- |
| Ethereum networks for settlement mirrors | Ethereum mainnet and its L2s | ethereum-sepolia (chain 11155111), base-sepolia (chain 84532), hood-testnet (chain 46630) per interop/deploy/mirror/ethereum-testnets-2026-08-31.json | ETHEREUM_RPC_A_TOKEN, ETHEREUM_RPC_B_TOKEN, ETHEREUM_CONFIG_B, MIRROR_LIVE_CONFIG, MIRROR_VERIFY_CONFIG |
| Solana network for the mirror program | Solana mainnet-beta | solana-devnet per interop/deploy/mirror/solana-devnet-2026-08-31.json | SOLANA_RPC_A_TOKEN, SOLANA_RPC_B_TOKEN, SOLANA_CONFIG_B |
| Paxeer custody boundary | Paxeer production network | the in-cluster paxd chain 125 node and its JSON-RPC boundary from platform/hosted/paxeer/deployment.yaml, reached through testnet_paxeer_url; platform/hosted/tests/beta-cluster.sh builds paxd from paxeer-network/ (Dockerfile.paxd-node), initialises the chain with the generated deployer address and deploys the settlement contracts with platform/hosted/paxeer/deploy-contracts.sh from the node genesis | LAYERX_BETA_FOUNDRY_BIN (pinned forge and cast); the deployer, controller and governance keys are generated by the bring-up |
| Pending-release in-cluster services (core, core-admin, identity, receipt authority, agent boundary) | production deployments of the same services | the same services on the beta cluster from platform/hosted/node/deployment.yaml (layerx-pending-core, layerx-pending-core-admin, layerx-receipt-authority, layerx-agent-boundary) and platform/hosted/identity/deployment.yaml (layerx-identity), addressed by testnet_core_url, testnet_core_admin_url, gateway_identity_url, gateway_authority_url and gateway_component_url; their images are built from the repository by platform/hosted/tests/beta-cluster.sh | LAYERX_BETA_SEQUENCER_KEY_FILE (generated when unset); the node, identity and service tokens are generated by the bring-up |
| Beta Kubernetes cluster | production cluster | owner-designated beta cluster or a disposable local cluster running the real manifests and images | KUBECONFIG |
| Ramp provider, compliance and KMS contracts | production layerx-ramp-provider-v1, layerx-ramp-compliance-v1 and KMS signature boundary | provider sandboxes | RAMP_URL, RAMP_OPERATOR_URL, RAMP_CA_PEM_B, RAMP_CUSTOMER_TOKEN, RAMP_OPERATOR_TOKEN, RAMP_ON_QUOTE_ID, RAMP_OFF_QUOTE_ID, RAMP_OFF_GRANT_JSON, RAMP_ON_ACCOUNT_SEQUENCE, RAMP_OFF_RECEIVER_SEQUENCE |
| Package registries | crates.io, npm, PyPI, Go module proxy, Maven Central, SwiftPM tags, NuGet | the same registries with beta pre-release versions, or owner-designated beta registries | secrets LAYERX_RELEASE_CRATES_IO_TOKEN, LAYERX_RELEASE_NPM_TOKEN (empty selects npm OIDC trusted publishing), LAYERX_RELEASE_GO_TAG_TOKEN, LAYERX_RELEASE_MAVEN_CENTRAL_TOKEN, LAYERX_RELEASE_NUGET_API_KEY, LAYERX_RELEASE_SWIFTPM_REMOTE, LAYERX_RELEASE_GPG_PRIVATE_KEY, LAYERX_RELEASE_GPG_PASSPHRASE; a PyPI trusted publisher for platform.yml; beta registry overrides LAYERX_RELEASE_CRATES_IO_INDEX, LAYERX_RELEASE_NPM_REGISTRY, LAYERX_RELEASE_PYPI_REPOSITORY_URL, LAYERX_RELEASE_PYPI_INDEX_URL, LAYERX_RELEASE_GO_PROXY, LAYERX_RELEASE_MAVEN_UPLOAD_URL, LAYERX_RELEASE_MAVEN_REPOSITORY_URL, LAYERX_RELEASE_SWIFTPM_URL, LAYERX_RELEASE_NUGET_SOURCE |
| Real agent-framework services | production RPC, budget, signer, receipt and A2A services | scheduled beta services | LAYERX_SCHEDULED_AGENT_RPC_URL, LAYERX_SCHEDULED_BUDGET_SERVICE_URL, LAYERX_SCHEDULED_SIGNER_SERVICE_URL, LAYERX_SCHEDULED_RECEIPT_SERVICE_URL, LAYERX_SCHEDULED_A2A_URL, LAYERX_SCHEDULED_AGENT_TOKEN, LAYERX_SCHEDULED_SPEND_TOOL_INPUT_JSON |
| Webhook delivery fixture | production webhook service | scheduled beta webhook service | LAYERX_SCHEDULED_WEBHOOK_FIXTURE_URL, LAYERX_SCHEDULED_WEBHOOK_FIXTURE_TOKEN |
| Hosted testnet CA | production CA | beta CA under the names in the Beta CA table | LAYERX_SCHEDULED_TESTNET_CA_BASE64 |
| macOS and Android toolchains | release build runners | owner-provided runners for ios-application-artifact and android-application-artifact | runner labels of .github/workflows/platform.yml |

## Beta-versus-production differences

| Key | Difference |
| --- | --- |
| ui_polish | UI polish is not a beta gate; human-qualify-ui is excluded from the functional bar. |
| visual_regression | Visual regression baselines (HUMAN_VISUAL_BASELINE_REVIEWED) are not a beta gate. |
| automated_accessibility | Automated accessibility scans are not a beta gate. |
| usability_studies | Usability studies (human-qualify-usability) are not a beta gate. |
| performance_budgets_and_soak | Performance budgets and soak runs (human-qualify-perf) are not a beta gate. |
| external_security_audit | No external security audit is required for the beta. |
| production_infrastructure | The beta runs on beta infrastructure: a beta cluster, test networks, sandboxes, the Paxeer testnet and pre-release registry versions, never production. |
| production_certification | The beta requires runtime_proven and deployment_proven rungs, not owner_certified. |

## Contradictions

| Key | Canonical value | Divergent source | Divergent value | Resolving task |
| --- | --- | --- | --- | --- |
| faucet_hostname | faucet.testnet.layerx.network | platform/hosted/testnet/deployment.yaml: Service layerx-faucet-public is a LoadBalancer with no Ingress host | (no ingress host) | 3.7 |
| placeholder_hostname | layerx.network | platform/hosted/webhooks/deployment.yaml Ingress layerx-developer and layerx-developer-web host | developers.layerx.example | 3.7 |

Each row records a value that a source carries today and that disagrees with the canonical value. The contract check recomputes every row from the sources; a row that disappears from the sources must be removed here, a disagreement that is not listed here fails the build, and the readiness claim cannot become `true` while any row remains.
