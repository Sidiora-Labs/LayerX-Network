# Paxeer Network documentation

These files document the **Paxeer Network** subtree of [Sidiora-Labs/LayerX-Protocol](https://github.com/Sidiora-Labs/LayerX-Protocol). They are not a standalone repository. The node, modules, RPC, and chain contracts live in `paxeer-network/`; LayerX execution, settlement contracts, and specifications live beside it at the monorepo root.

Paxeer is the EVM L1 (chain ID `125`) LayerX uses for custody and checkpoint settlement. Co-location in this monorepo does not merge the two trust boundaries.

| Document | Contents |
| --- | --- |
| [`../README.md`](../README.md) | What this subtree is, how it sits next to LayerX, build and test entrypoints |
| [`evm_jsonrpc_unsupported.md`](evm_jsonrpc_unsupported.md) | EVM JSON-RPC methods registered on Pax that return a documented error |
| This page (below) | Regenerating the node's OpenAPI / Swagger UI |
| [`../docker/README.md`](../docker/README.md) | Local Docker node and cluster |
| [`../contracts/README.md`](../contracts/README.md) | Foundry / Hardhat for Paxeer-native contracts |
| [`../modules/README.md`](../modules/README.md) | Chain modules under `modules/` |
| [`../hpx/README.md`](../hpx/README.md) | `paxd` distribution and peer registry tooling |

LayerX settlement contracts (custody, checkpoints, bonds, exits) are documented with the protocol at the repository root (`contracts/`, `spec/layerx-protocol/`). They are not this subtree's `contracts/` directory.

Commands below are run from `paxeer-network/` unless noted.

---

# OpenAPI/Swagger docs generation

> **Note:** Anytime we make changes to the APIs/proto files, we also need to update the Swagger/OpenAPI docs.

Both Swagger and OpenAPI terms are used interchangeably in this document.

- [OpenAPI](https://github.com/OAI/OpenAPI-Specification/) = specification
- Swagger = Tools for implementing the specification.

The process of Swagger docs generation involves running a script that does the following tasks:

1. Generating the swagger yml file using the `ignite` tool.
2. Updating the static assets for the Swagger UI.

## Prerequisites

The docs generation script uses the `ignite` tool to generate the OpenAPI docs.
So, first install the `ignite` tool, if not installed already.
We need version v0.23.0, which is outdated, but works with the current version of the codebase.
Pull binaries from the [releases page](https://github.com/ignite/cli/releases/tag/v0.23.0) or install from source code
following instructions.

Verify the installation by running `ignite version`:

```bash
% ignite version
·
· 🛸 Ignite CLI v28.2.0 is available!
·
· To upgrade your Ignite CLI version, see the upgrade doc: https://docs.ignite.com/guide/install.html#upgrading-your-ignite-cli-installation
·
·

Ignite CLI version:     v0.23.0
....

```

## Running the script

Then run the following command to generate the OpenAPI docs and update static assets for Swagger UI:

```bash
./scripts/update-swagger-ui-statik.sh
```

The script generates a new swagger/openapi yml and saves it as `docs/swagger-ui/swagger.yml` under this subtree.
It will then embed the yml file and static assets in `docs/swagger-ui/` into the `docs/swagger/statik.go` file.

If swagger endpoint is configured, `paxd` on startup will read `statik.go` and make its content available at the
`/swagger/` endpoint.

# Serving the OpenAPI docs on the node

To start serving the docs, enable the swagger docs serving.
To do that, update `api.swagger` value to `true` in the `app.toml` file.

```toml
###############################################################################
###                           API Configuration                             ###
###############################################################################

[api]

# Enable defines if the API server should be enabled.
enable = true

# Swagger defines if swagger documentation should automatically be registered.
swagger = true
```

Once the node is restarted, swagger docs will be available at `http://<node-ip>:<port>/swagger/`.
