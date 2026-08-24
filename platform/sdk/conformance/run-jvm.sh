#!/bin/sh
set -eu

repo_root=${1:-.}
repo_root=$(cd "$repo_root" && pwd)

cargo run --manifest-path "$repo_root/platform/Cargo.toml" --locked \
	-p layerx-platform-sdkgen -- --check "$repo_root"
mvn -q -f "$repo_root/platform/sdk/jvm/pom.xml" -Pconformance test \
	exec:java -Dexec.classpathScope=test \
	-Dexec.mainClass=com.sidiora.layerx.sdk.ConformanceMain \
	-Dexec.args="$repo_root"
