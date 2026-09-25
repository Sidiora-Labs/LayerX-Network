# Reference

Operator-facing and encoding-heavy pages collected from the former wiki
and from `docs/`.

## Public contracts

- [Public RPC endpoints](public-rpc.md)
- [Public JSON-RPC](../platform/gateway-rpc.md)
- [Public payment API transcript](../platform/public-api.md)
- [CLI](../platform/cli.md)
- [Assets and tokens](../concepts/assets.md)
- [Commitment levels](../protocol/commitment-levels.md)
- [Modules](../protocol/modules.md)

## Specifications

Normative text is KVX first. Generated `requirements.md`, `design.md`, and
`tasks.md` sit next to each `spec.kvx`. Do not hand-edit those generated
files.

| Feature | Checked-in path | Status in `[meta]` |
| --- | --- | --- |
| Protocol | `spec/layerx-protocol/spec.kvx` | active |
| Platform (includes the human plane) | `spec/layerx-platform/spec.kvx` | active |
| Beta remediation | `spec/layerx-beta/spec.kvx` | draft |
| Agent interface | `spec/.beta/layerx-agent-interface/spec.kvx` | draft |

The platform and beta specifications refer to the agent-interface document
as `spec/layerx-agent-interface`. That path is not present at the repository
root; the checked-in file is under `spec/.beta/`.

The platform specification supersedes `spec/layerx-human-interface` as the
active feature and carries its requirements and tasks verbatim.

## Qualification and layout

- [Qualification](../operators/qualification.md)
- [Monorepo](../overview/monorepo.md)
- [Translations](../overview/translations.md)

## Contributing and security

- [CONTRIBUTING.md](https://github.com/Sidiora-Labs/Paxeer-X-Network/blob/main/CONTRIBUTING.md)
- [SECURITY.md](https://github.com/Sidiora-Labs/Paxeer-X-Network/blob/main/SECURITY.md)
- [LICENSE](https://github.com/Sidiora-Labs/Paxeer-X-Network/blob/main/LICENSE)
