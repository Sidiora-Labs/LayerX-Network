# Agents

The LayerX Agent Interface is the Rust interaction layer through which
autonomous agents reach the protocol
(`spec/.beta/layerx-agent-interface/spec.kvx`). It never invents or directly
changes protocol state. Every mutation becomes canonical signed LayerX bytes
submitted to the core. Every claimed result is backed by a core-produced
receipt or proof that this layer verified for itself.

| Topic | Page |
| --- | --- |
| LayerX Node Interface | [LNI](lni.md) |
| Agent daemon | [Agentd](agentd.md) |
| Enrolment and MCP host path | [Running an agent](running.md) |
| Language SDKs | [SDKs](sdks.md) |
| MCP tools | [MCP](mcp.md) |
| Program CALL terminals | [SDK terminal verification](sdk-verification.md) |

`layerx-agentd` is `agent/crates/layerx-agentd`. The MCP server is
`agent/crates/layerx-mcp`. The developer CLI installs those transports with
`layerx install mcp` and `layerx install a2a`.
