PLATFORM_CARGO ?= cargo
PLATFORM_MANIFEST := platform/Cargo.toml

.PHONY: platform-build platform-lint platform-test platform-sdk-check \
	platform-sdk-generate platform-test-sdks platform-release-check platform-release-plan \
	platform-test-tooling platform-emulator-conformance platform-test-middleware \
	platform-test-docs

platform-build:
	$(PLATFORM_CARGO) build --manifest-path $(PLATFORM_MANIFEST) --locked --workspace
	$(MAKE) platform-sdk-check
	$(PLATFORM_CARGO) run --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-release -- --check platform/release/registries.kvx

platform-lint:
	$(PLATFORM_CARGO) clippy --manifest-path $(PLATFORM_MANIFEST) --locked --workspace --all-targets -- -D warnings
	$(PLATFORM_CARGO) run --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-workspace-gate -- platform/workspace.kvx .
	sh platform/tools/dependency-policy.sh
	cargo deny --manifest-path $(PLATFORM_MANIFEST) check advisories bans sources

platform-test:
	$(PLATFORM_CARGO) test --manifest-path $(PLATFORM_MANIFEST) --locked --workspace

platform-test-tooling:
	$(PLATFORM_CARGO) check --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-cli
	$(PLATFORM_CARGO) check --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-emulator
	$(PLATFORM_CARGO) test --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-faucet
	$(PLATFORM_CARGO) test --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-testnet
	sh -n platform/hosted/testnet/run-testnet.sh platform/hosted/testnet/reset-testnet.sh \
		platform/hosted/testnet/render-status.sh platform/hosted/testnet/tests/hosted-smoke.sh
	@if test -n "$${LAYERX_TESTNET_URL:-}" && test -n "$${LAYERX_CONFORMANCE_ACTIVITIES:-}"; then \
		sh platform/emulator/tests/conformance.sh \
			"$${LAYERX_EMULATOR_URL:-http://127.0.0.1:9402}" \
			"$$LAYERX_TESTNET_URL" "$${LAYERX_CONFORMANCE_ACTIVITIES}"; \
	else \
		echo "emulator/testnet differential skipped: set LAYERX_TESTNET_URL and LAYERX_CONFORMANCE_ACTIVITIES"; \
	fi
	@if test -n "$${LAYERX_TESTNET_URL:-}" && test -n "$${LAYERX_FAUCET_URL:-}" && \
		test -n "$${LAYERX_TEST_IDENTITY:-}" && test -n "$${LAYERX_TEST_PUBLIC_KEY:-}" && \
		test -n "$${LAYERX_TEST_DID:-}" && test -n "$${LAYERX_CONFORMANCE_ACTIVITIES:-}"; then \
		sh platform/hosted/testnet/tests/hosted-smoke.sh; \
	else \
		echo "hosted testnet/faucet smoke skipped: set scheduled CI hosted-testnet variables"; \
	fi

platform-emulator-conformance:
	@test -n "$${LAYERX_TESTNET_URL:-}" && test -n "$${LAYERX_CONFORMANCE_ACTIVITIES:-}"
	sh platform/emulator/tests/conformance.sh \
		"$${LAYERX_EMULATOR_URL:-http://127.0.0.1:9402}" \
		"$$LAYERX_TESTNET_URL" "$${LAYERX_CONFORMANCE_ACTIVITIES}"

platform-sdk-check:
	python3 platform/sdk/generators/generate_portable.py --check $(CURDIR)
	$(PLATFORM_CARGO) run --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-sdkgen -- --check

platform-sdk-generate:
	cargo run --manifest-path agent/tools/sdk-gen/Cargo.toml --locked -- --write
	cargo run --manifest-path human/tools/api-gen/Cargo.toml --locked -- human/schema/human-api human/apps/web/src/api/generated
	python3 platform/sdk/generators/generate_portable.py $(CURDIR)
	$(PLATFORM_CARGO) run --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-sdkgen -- --write

platform-test-sdks: platform-sdk-check
	sh platform/sdk/conformance/run-go.sh $(CURDIR)
	sh platform/sdk/conformance/run-jvm.sh $(CURDIR)
	command -v swift >/dev/null
	swift build --package-path platform/sdk/swift
	command -v dotnet >/dev/null
	dotnet build platform/sdk/dotnet/LayerX.Sdk.csproj --configuration Release --nologo

platform-release-check:
	$(PLATFORM_CARGO) run --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-release -- --check platform/release/registries.kvx

platform-release-plan:
	$(PLATFORM_CARGO) run --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-release -- --plan platform/release/registries.kvx

platform-test-middleware:
	cd platform/middleware/seller && npm install --no-audit --no-fund && npm run build
	cd platform/middleware/buyer && npm install --no-audit --no-fund && npm run build
	cd platform/middleware/conformance && npm install --no-audit --no-fund && npm run build && npm run conformance
	cd platform/integrations/express && npm install --no-audit --no-fund && npm run build
	cd platform/integrations/next && npm install --no-audit --no-fund && npm run build
	node platform/integrations/next/bin/layerx-scan-bundle.mjs platform/integrations/next/example
	PYTHONPATH=agent/sdk/python:platform/integrations/fastapi python3 -m compileall -q \
		platform/integrations/fastapi/layerx_fastapi platform/integrations/fastapi/examples
	mvn -q -f platform/sdk/jvm/pom.xml -DskipTests install
	mvn -q -f platform/integrations/spring/pom.xml -DskipTests install
	cd platform/middleware/agent && npm install --no-audit --no-fund && npm run build
	cd platform/integrations/agents && npm install --no-audit --no-fund && npm run build
	mvn -q -f platform/integrations/android/pom.xml -DskipTests verify
	command -v swift >/dev/null
	swift build --package-path platform/integrations/ios

platform-test-docs:
	sh platform/tools/docs-gate $(CURDIR)
	command -v node >/dev/null
	npm install --no-audit --no-fund \
		--workspace @sidiora/layerx-doc-first-payment-typescript \
		--workspace @sidiora/layerx-doc-paid-endpoint-express \
		--workspace @sidiora/layerx-doc-paid-route-next
	python3 -m pip install --quiet -r platform/docs/samples/first-payment-python/requirements.txt
	python3 -m pip install --quiet -r platform/docs/samples/paid-endpoint-fastapi/requirements.txt
	python3 -m compileall -q platform/docs/samples/first-payment-python platform/docs/samples/paid-endpoint-fastapi
	cd platform/docs/samples/first-payment-go && go build ./...
	cargo build --manifest-path platform/docs/samples/verify-receipt-rust/Cargo.toml --release
	command -v mvn >/dev/null
	mvn -q -f platform/docs/samples/first-payment-jvm/pom.xml package
	mvn -q -f platform/docs/samples/paid-endpoint-spring/pom.xml package
	mvn -q -f platform/docs/samples/mobile-payment-android/pom.xml package
	command -v swift >/dev/null
	swift build --package-path platform/docs/samples/first-payment-swift
	swift build --package-path platform/docs/samples/mobile-payment-ios
	command -v dotnet >/dev/null
	dotnet build platform/docs/samples/first-payment-csharp/FirstPayment.csproj --configuration Release --nologo
