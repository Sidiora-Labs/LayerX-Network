# SDKs

The platform specification (`spec/layerx-platform/spec.kvx`, decision
`sdk_single_schema` and requirement 24) requires every SDK — Rust, TypeScript,
Python, Go, Java/Kotlin, Swift, C# — to be produced from the same `agent-api`
and `human-api` schemas in the same build. Idiom differs per language. Wire
behaviour, error taxonomy, and receipt verification are identical.
Hand-editing generated output is a build failure.

The agent-interface specification authors the Rust SDK and generates
TypeScript and Python from the same contract schema
(`spec/.beta/layerx-agent-interface/spec.kvx`, decision `sdk_generation`).
The platform specification extends that family with Go, JVM, Swift, and C#.

## Trees in this repository

| Language | Path |
| --- | --- |
| Rust | `agent/crates/layerx-sdk` |
| Python | `agent/sdk/python` |
| TypeScript | `agent/sdk/typescript` |
| Go | `platform/sdk/go` |
| JVM | `platform/sdk/jvm` |
| .NET | `platform/sdk/dotnet` |
| Swift | `platform/sdk/swift` |

The root README lists that table. Client SDKs under `platform/sdk/` also
appear in [Programs](../programs/index.md) for CALL receipt terminals.

## What an SDK may do

The agent-interface non-authority rule applies: an SDK constructs canonical
activities, obtains a disclosure-bound signature, submits those exact bytes,
and returns only values decoded from core-produced bytes with a verification
status. It does not apply transfers, derive state roots, or treat admission
as execution.

Program CALL terminal verification for the generated clients is
[SDK terminal verification](sdk-verification.md).

## CLI and RPC

The developer CLI in `platform/cli` is [CLI](../platform/cli.md). Public
JSON-RPC methods are [Public JSON-RPC](../platform/gateway-rpc.md).
`layerx --rpc` and `--gateway-credential` apply to `wallet`, `token`, and
`program`.

## Publication (beta)

The beta specification (`spec/layerx-beta/spec.kvx`, requirement 8) requires
the release workflow to publish every declared ecosystem under a beta
pre-release version with digest, signature, SBOM, and provenance attestation.
The artifact manifest is the only list of installable artifacts. Until that
gate is recorded on a named release-candidate revision, treat published
package versions as unspecified here.

## Status

The seven language trees are present. Cross-language parity, the ten-line
integration benchmark, and the five-minute first-payment gate are platform
qualification requirements (`spec/layerx-platform/spec.kvx`). The beta
feature records those as executed-evidence gates; they are not claimed as
passed from this documentation.
