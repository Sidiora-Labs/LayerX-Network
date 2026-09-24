Env collector
===

This is a simple tool to collect possible environment variables from `Settings` structure and place it to `README.md` file.

## Usage

1. Add `env-collector` to your `server` crate:
    ```toml
    # Cargo.toml
    [dependencies]
    env-collector = { git = "https://github.com/blockscout/blockscout-rs", version = "0.1.1" }
    ```
2. In your `server` crate create new binary file called `check-envs.rs` with the following content:

    ```rust
    // check-envs.rs
    use <path_to_settings>::Settings;
    use env_collector::{run_env_collector_cli, EnvCollectorSettingsBuilder, PrefixFilter};
    
    fn main() {
        run_env_collector_cli::<Settings>(
            EnvCollectorSettingsBuilder::default()
                .service_name("<SERVICE_NAME_PREFIX>")
                .markdown_path("README.md")
                .config_path("<PATH TO .TOML/.JSON EXAMPLE CONFIG>")
                .vars_filter(PrefixFilter::blacklist(&[
                    "<ENV_PREFIX_TO_IGNORE>",
                    "<SERVICE_NAME>__SERVER",
                    "<SERVICE_NAME>__JAEGER",
                    "<SERVICE_NAME>__METRICS",
                    "<SERVICE_NAME>__TRACING"
                ]))
                .anchor_postfix(Some("some_postfix".to_string()))
                .build()
                .expect("failed to build env collector settings"),
        );
    }
    ```
3. In `README.md` file add special **anchors** lines to specify where to store the table with ENVs:

    ```markdown
    ## Envs

    [anchor]: <> (anchors.envs.start.some_postfix)
    [anchor]: <> (anchors.envs.end.some_postfix)
    ```

4. (Optional) In your `justfile` add new commands to run `check-envs.rs`:

    ```just
    # justfile
    check-envs *args:
        cargo run --bin env-docs-generation -- --validate-only {{args}}

    generate-envs *args:
        cargo run --bin env-docs-generation -- {{args}}
    ```

5. Run command `just generate-envs` (or `cargo run --bin env-docs-generation`) to generate ENVs table in `README.md` file.

6. Add github action to run this binary on every push to validate ENVs table in `README.md` file:    
    ```yaml
    [... other steps of `test` job ...]
    
    - name: Verify ENVs in README
      run: cargo run --bin check-envs -- --validate-only
    
    [... other steps of `test` job ...]
      ```
