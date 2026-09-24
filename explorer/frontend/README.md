<h1 align="center">Blockscout frontend</h1>

<p align="center">
    <span>Frontend application for </span>
    <a href="https://github.com/blockscout/blockscout/blob/master/README.md">Blockscout</a>
    <span> blockchain explorer</span>
</p>

## Running and configuring the app

App is distributed as a docker image. Here you can find information about the [package](https://github.com/blockscout/frontend/pkgs/container/frontend) and its recent [releases](https://github.com/blockscout/frontend/releases).

You can configure your app by passing necessary environment variables when starting the container. See full list of ENVs and their description [here](./docs/ENVS.md).

```sh
docker run -p 3000:3000 --env-file <path-to-your-env-file> ghcr.io/blockscout/frontend:latest
```

Alternatively, you can build your own docker image and run your app from that. Please follow this [guide](./docs/CUSTOM_BUILD.md).

For more information on migrating from the previous frontend, please see the [frontend migration docs](https://docs.blockscout.com/setup/deployment/frontend-migration).

## Paxeer X

This tree is deployed as the Paxeer X Network explorer. Two run-time ENV presets are tracked under [`configs/envs`](./configs/envs):

- `paxeer-x.env` — the public deployment (https, wss).
- `paxeer-x-dev.env` — the development deployment (http, ws).

Both describe network `Paxeer X Network` (short name `Paxeer X`, network id `125`, native coin `PAX`, 18 decimals), carry the Paxeer X marks from [`public/static/paxeer-x`](./public/static/paxeer-x), and link out to nothing except `https://paxeer.app` and the monorepo on GitHub. Marketplace, ads and third-party analytics are off.

Using a preset:

1. Copy it to a dotfile name the container entrypoint understands, e.g. `cp configs/envs/paxeer-x.env configs/envs/.env.paxeer-x`, and build or run with `ENVS_PRESET=paxeer-x`.
2. Replace the two deployment inputs in the copy: `REPLACE_API_HOST` (Blockscout API host) and `REPLACE_APP_HOST` (host the explorer itself is served from). `NEXT_PUBLIC_APP_*` is on the entrypoint's preset blacklist, so the app host can also be supplied straight from the container environment.

Validating a preset without a full build:

```sh
cd deploy/tools/envs-validator
yarn install --frozen-lockfile
NEXT_PUBLIC_GIT_COMMIT_SHA=$(git rev-parse --short HEAD) NEXT_PUBLIC_GIT_TAG=$(git describe --tags --always --abbrev=0) ../../scripts/collect_envs.sh ../../../docs/ENVS.md
yarn build
./node_modules/.bin/dotenv -e ../../../configs/envs/paxeer-x.env yarn run validate
```

## Contributing

See our [Contribution guide](./docs/CONTRIBUTING.md) for pull request protocol. We expect contributors to follow our [code of conduct](./CODE_OF_CONDUCT.md) when submitting code or comments.

## Resources
- [App ENVs list](./docs/ENVS.md)
- [Contribution guide](./docs/CONTRIBUTING.md)
- [Making a custom build](./docs/CUSTOM_BUILD.md)
- [Frontend migration guide](https://docs.blockscout.com/setup/deployment/frontend-migration)
- [Manual deployment guide with backend and microservices](https://docs.blockscout.com/setup/deployment/manual-deployment-guide)

## License

[![License: GPL v3.0](https://img.shields.io/badge/License-GPL%20v3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

This project is licensed under the GNU General Public License v3.0. See the [LICENSE](LICENSE) file for details.
