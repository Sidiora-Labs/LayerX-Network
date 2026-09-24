# Code generation

Protobuf code for the API and chain modules is generated with `buf`, configured by
[`buf.yaml`](../buf.yaml) and [`buf.gen.yaml`](../buf.gen.yaml) at the repository root.

To regenerate the code, run the following command from the repository root:

```bash
./scripts/protoc.sh
```

The script builds the pinned `protoc-gen-gocosmos` plugin, runs `buf generate` (via
`go run github.com/bufbuild/buf/cmd/buf`) against the root and `consensus/internal`
templates, and copies the generated Go files into their module directories.
