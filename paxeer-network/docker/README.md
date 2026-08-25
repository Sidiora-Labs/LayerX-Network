## Prerequisite

### Install Docker and Docker Compose
MacOS:
```sh
# The easiest and recommended way to get Docker and
# Docker Compose is to install Docker Desktop here:
https://docs.docker.com/desktop/install/mac-install/
```

Ubuntu:
```sh
# Follow the below link to install docker on ubuntu
https://docs.docker.com/engine/install/ubuntu/#install-using-the-repository
# Follow the below link to install standalone docker compose
https://docs.docker.com/compose/install/other/
```

## Local Cluster

Detailed instruction: see the `Makefile` in the root of [the repo](https://github.com/sidiora-labs/paxeer-network/blob/main/Makefile)

**To start a single local node (Not Recommended)**

```sh
make build-docker-node && make run-local-node
```

**To start 4 node cluster**

This will start a 4 node pax chain cluster.
```sh
# If this is the first time or you want to rebuild the binary:
make docker-cluster-start

# If you have run docker-cluster-start and build/paxd exist,
# you can skip the build process to quick start by:
make docker-cluster-start-skipbuild
```
All the logs and genesis files will be generated under the temporary build/generated folder.

```sh
# To monitor logs after cluster is started
tail -f build/generated/logs/paxd-0.log
```

**To ssh into a single node**
```sh
# List all containers
docker ps -a
# SSH into a running container
docker exec -it [container_name] /bin/bash
```

## Prometheus / Grafana (monitornode)

**Cluster and monitoring together:** from the repo root you can run:

```sh
make docker-cluster-start-monitoring
```

This stops any existing compose stack, rebuilds the node image, starts the four-node local cluster, and brings up the Prometheus and Grafana containers via the monitoring compose overlay, so you do not need to run the scripts below for that flow. To tear down the cluster and monitoring containers together:

```sh
make docker-cluster-stop-monitoring
```

**Scripts only:** to start Prometheus and Grafana by themselves (for example when you are not using the make target above):

```sh
./docker/monitornode/scripts/start-prometheus.sh
./docker/monitornode/scripts/start-grafana.sh
```

Grafana UI: http://localhost:3000 (login: admin / admin). To stop containers started via the scripts:

```sh
./docker/monitornode/scripts/stop-prometheus.sh
./docker/monitornode/scripts/stop-grafana.sh
```

## State Sync RPC Node

Requirement: Follow the above steps to start a 4 node docker cluster before starting any state sync node

```sh
# Be sure to start up a 4-node cluster before you start a state sync node
make docker-cluster-start
# Wait for at least a few minutes till the latest block height exceed 500 (this can be changed via app.toml)
paxd status |jq
# Start up a state sync node
make run-rpc-node
```

## Local Debugging & Testing
One of the benefits of using Docker is fast iteration. This setup supports:
- Being able to make changes locally and start up the chain to see the immediate impact
- Being able to change the vendored Cosmos SDK, Tendermint, database, and Wasm modules in this repository without publishing separate module versions

The dependencies are part of this monorepo, so no sibling repositories or `go.mod` replacements are required:
```sh
# Edit sdk/, consensus/, storage/, wasm/, or wasm-runtime/, then rebuild.
make build-docker-node
```
****



# Build with Us!
If you are interested in building with Pax Network:
Email us at team@paxnetwork.io
DM us on Twitter https://twitter.com/PaxNetwork
