# Development status

## Current status: Limited beta

LayerX is under active development and release qualification.

| Milestone | Status | Date |
| --- | --- | --- |
| Source publication | ✅ Complete | August 2026 |
| Limited beta opens | 🔜 Upcoming | September 7, 2026 |
| Public RPC endpoint | ⏳ Pending | After qualification |
| Open-source license | ⏳ Pending | After qualification |

## What is available now

- **Source code**: Open for inspection and security review under temporary source-available terms
- **Repository**: Full LayerX + Paxeer monorepo at [github.com/Sidiora-Labs/LayerX-Protocol](https://github.com/Sidiora-Labs/LayerX-Protocol)
- **Documentation**: Protocol specifications, design documents, contributing guidelines, and qualification evidence
- **Build system**: Complete build, test, and qualification infrastructure

## Monorepo layout

This repository contains both LayerX and Paxeer Network as a unified ecosystem monorepo:

- **LayerX** builds, releases, and qualifies independently at the repository root
- **Paxeer Network** builds, releases, and qualifies independently under `paxeer-network/`
- See `docs/MONOREPO.md` for details on build boundaries, release tags, and trust separation

## What is not yet available

- **Public RPC endpoint**: No public LayerX node is accessible yet
- **Production deployment**: No mainnet or production environment is live
- **Deployment license**: Current license permits inspection only, not deployment or redistribution

## Limited beta (September 7, 2026)

Limited beta opens September 7, 2026. Limited beta does not mean production-ready. The system remains under qualification, and breaking changes may still occur.

## Fees

LayerX charges a base fee of 5,000 µUSDX per activity (approximately ½¢) for network operation, sequencing, and data availability. Congestion applies a 1×–64× multiplier measured from network load. This is not zero-fee; it reflects the real cost of operating a deterministic accounting network with data availability and replay guarantees.

## Qualification status

LayerX qualification is layered by risk. The authoritative acceptance criteria and current completion state are in [docs/QUALIFICATION.md](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/docs/QUALIFICATION.md) and [spec/layerx-protocol/tasks.md](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/spec/layerx-protocol/tasks.md).

Paxeer has its own independent qualification gates (`make paxeer-ci`, `make monorepo-ci`). Paxeer qualification is separate from LayerX qualification—one does not imply the other.

## License timeline

LayerX is currently available under temporary source-available terms that permit inspection and security review but do not grant deployment rights.

Sidiora Labs intends to publish LayerX under an open-source license after protocol development and release qualification are complete. No timeline has been announced for that transition.

## How to participate

**Now:**
- Inspect the source code
- Review the specifications and design documents
- Report security issues via [SECURITY.md](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/SECURITY.md)

**After qualification:**
- Use LayerX in production environments
- Deploy your own nodes (when deployment license is available)
- Build agent applications on top of LayerX

---

LayerX is developed by [Sidiora Labs](https://github.com/Sidiora-Labs). For questions or support, see [SUPPORT.md](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/SUPPORT.md).
