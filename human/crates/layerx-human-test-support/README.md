# Human service test support

Shared development helpers for the human-service integration suites. The library installs principal stores, encodes signed receipt and state evidence with workspace cryptography, and serves the existing Unix admission journal fixture. Its callers retain their independent Cargo integration target names.

The durable admission fixture verifies encoded activity hashes and signatures, records accepted activities, and exposes journal recovery and refusal counters to its integration tests. It does not substitute for live native-node or settlement qualification.
