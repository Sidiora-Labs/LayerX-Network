# Release Workflow

To make a new release do the following:

1. Create a new branch for the release. Name it e.g. `publish-0.11.0`.
1. Bump versions in `/Cargo.toml` and `/binaryen-sys/Cargo.toml`. Also don't forget to bump `binaryen-sys`
dependency in `/Cargo.toml`.
1. Commit the changes, push them and open a Pull Request.
1. If it turned green:
    1. publish `binaryen-sys` and then `binaryen`.
    1. create a tag. It would look something like `0.11.0`
    1. merge the PR
