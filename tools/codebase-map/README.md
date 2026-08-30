# LayerX Codebase Map

An offline, navigable rendering of the current LayerX checkout. It combines the Git file inventory and working-tree state with Codify symbols, resolved calls, imports and routes, plus the authoritative LayerX Platform task dependency graph.

From the repository root:

```bash
cg sync
npm --prefix tools/codebase-map run generate
npm --prefix tools/codebase-map run serve
```

Open `http://127.0.0.1:4177`. The generated data stays in `tools/codebase-map/data/` and is intentionally ignored by Git. Set `LAYERX_CODE_MAP_PORT` to use another port; the server binds to localhost unless `LAYERX_CODE_MAP_HOST` is explicitly set.

## What the map proves

- Every Git-tracked path is present in the file inventory, including paths excluded from semantic indexing.
- Files in `.codegraph/graph.db` include their exact indexed language, line, hash, symbol and reference metadata.
- Internal call edges exist only where Codify records a concrete `target_id`.
- Internal import edges exist only where Codify records a concrete `target_file_id`.
- Package views are manifest scopes: a file is grouped under its nearest ancestor manifest, and scope-to-scope edges are aggregated from resolved file edges.
- External, receiver-only, ambiguous and unresolved references remain visible as coverage counts; the viewer does not invent targets for them.
- Tasks, statuses, waves, requirements, declared symbols, touch paths and dependency edges are parsed from `spec/layerx-platform/spec.kvx` at generation time.
- The header records the Git commit, branch, dirty-file count, Codify schema and generation time for the snapshot being viewed.

The graph is a faithful rendering of its declared sources, not a claim that a static parser understands runtime behavior. The coverage strip and each file inspector make the indexed-versus-tree-only boundary visible.

## Navigation

- Start in **Systems**, then double-click an area, package or file to drill toward symbols.
- Use **Imports** to see unresolved/external modules globally or the exact imports for a selected file.
- Use **Routes** for detected framework entry points and **Delivery** for task dependencies and qualification commands.
- Search reaches every file and symbol even when a dense graph view limits the number of nodes drawn at once.
- Press `/` to search, `F` to fit, `Escape` to clear and `Left Arrow` to return to the previous scope.

Run the structural and source-consistency checks after generation:

```bash
npm --prefix tools/codebase-map run check
```
