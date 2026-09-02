PLATFORM_CARGO ?= cargo
PLATFORM_MANIFEST := platform/Cargo.toml
PLATFORM_BETA_CLUSTER_ENV ?= build/beta-cluster/env

.PHONY: platform-js-install platform-build platform-build-all platform-lint platform-test platform-sdk-check \
	platform-sdk-generate platform-receipt-fixture platform-verify-sdks platform-test-sdks platform-release-check platform-release-plan platform-release-test \
	platform-test-tooling platform-emulator-conformance platform-test-middleware \
	platform-test-cli-production-credential-refusal platform-test-registry \
	platform-test-reference-apps platform-test-mobile-artifacts \
	platform-test-android-artifact platform-test-ios-artifact \
	platform-real-agent-integration \
	platform-real-ios-integration platform-real-android-integration \
	platform-test-docs platform-hosted-smoke platform-hosted-topology-check platform-beta-cluster-up platform-beta-cluster-down \
	platform-test-agent-install \
	platform-policy-install platform-sample-preflight platform-sample-install platform-sample-dependencies-ready \
	platform-install-local-artifacts platform-dependencies-install platform-gradle-qualification

PLATFORM_PYTHON_ENV := $(CURDIR)/build/platform-python
PLATFORM_MAVEN_REPOSITORY ?= $(HOME)/.m2/repository

platform-policy-install:
	@cargo deny --version >/dev/null || { echo "cargo-deny is required for Platform policy checks" >&2; exit 1; }
	cargo deny fetch

platform-sample-preflight:
	@command -v python3 >/dev/null || { echo "python3 is required for Platform samples" >&2; exit 1; }
	@command -v go >/dev/null || { echo "Go is required for Platform samples" >&2; exit 1; }
	@command -v cargo >/dev/null || { echo "Cargo is required for Platform samples" >&2; exit 1; }
	@command -v mvn >/dev/null || { echo "Maven is required for Platform samples" >&2; exit 1; }
	@command -v swift >/dev/null || { echo "Swift is required for Platform SDK and sample dependency resolution" >&2; exit 1; }
	@command -v dotnet >/dev/null || { echo "dotnet is required for the C sharp sample" >&2; exit 1; }

platform-sample-install: platform-sample-preflight
	python3 -m venv $(PLATFORM_PYTHON_ENV)
	$(PLATFORM_PYTHON_ENV)/bin/python -m pip install --no-index --no-deps --no-build-isolation \
		$(CURDIR)/agent/sdk/python $(CURDIR)/platform/integrations/fastapi
	cd platform/sdk/go && go mod download
	cd platform/docs/samples/first-payment-go && go mod download
	cargo fetch --manifest-path platform/docs/samples/verify-receipt-rust/Cargo.toml --locked
	cargo fetch --manifest-path platform/examples/marketplace/program/Cargo.toml --locked
	@set -eu; for pom in platform/sdk/jvm/pom.xml platform/integrations/spring/pom.xml platform/integrations/android/pom.xml \
		platform/integrations/spring/example/pom.xml \
		platform/docs/samples/first-payment-jvm/pom.xml platform/docs/samples/paid-endpoint-spring/pom.xml \
		platform/docs/samples/mobile-payment-android/pom.xml; do \
		mvn -q -f "$$pom" -Pconformance dependency:go-offline -DexcludeGroupIds=com.sidiora.layerx; \
	done
	@set -eu; \
		sdk_lock=$$(sha256sum platform/sdk/swift/Package.resolved | awk '{print $$1}'); \
		ios_lock=$$(sha256sum platform/integrations/ios/Package.resolved | awk '{print $$1}'); \
		swift package --package-path platform/sdk/swift --force-resolved-versions resolve; \
		swift package --package-path platform/integrations/ios --force-resolved-versions resolve; \
		test "$$(sha256sum platform/sdk/swift/Package.resolved | awk '{print $$1}')" = "$$sdk_lock"; \
		test "$$(sha256sum platform/integrations/ios/Package.resolved | awk '{print $$1}')" = "$$ios_lock"
	dotnet restore platform/sdk/dotnet/LayerX.Sdk.csproj --nologo
	dotnet restore platform/sdk/dotnet/tests/LayerX.Sdk.Tests/LayerX.Sdk.Tests.csproj --nologo
	dotnet restore platform/docs/samples/first-payment-csharp/FirstPayment.csproj --nologo

platform-sample-dependencies-ready:
	@test -x $(PLATFORM_PYTHON_ENV)/bin/python || { echo "run make platform-sample-install first" >&2; exit 1; }
	@$(PLATFORM_PYTHON_ENV)/bin/python -c 'import importlib.metadata as m; assert m.version("layerx-sdk") == "0.1.0"; assert m.version("layerx-fastapi") == "0.1.0"'
	@test -s platform/docs/samples/verify-receipt-rust/Cargo.lock
	@command -v mvn >/dev/null || { echo "Maven is required for Platform samples" >&2; exit 1; }
	@command -v swift >/dev/null || { echo "Swift is required for Platform SDK and samples" >&2; exit 1; }
	@command -v dotnet >/dev/null || { echo "dotnet is required for the C sharp sample" >&2; exit 1; }
	@test -s $(PLATFORM_MAVEN_REPOSITORY)/com/sidiora/layerx/layerx-sdk/0.1.0/layerx-sdk-0.1.0.jar || { echo "run make platform-dependencies-install to install the local JVM SDK" >&2; exit 1; }
	@test -s $(PLATFORM_MAVEN_REPOSITORY)/com/sidiora/layerx/layerx-spring-boot-starter/0.1.0/layerx-spring-boot-starter-0.1.0.jar || { echo "run make platform-dependencies-install to install the local Spring integration" >&2; exit 1; }
	@test -s $(PLATFORM_MAVEN_REPOSITORY)/com/sidiora/layerx/layerx-android/0.1.0/layerx-android-0.1.0.jar || { echo "run make platform-dependencies-install to install the local Android integration" >&2; exit 1; }

platform-install-local-artifacts:
	mvn -o -q -f platform/sdk/jvm/pom.xml -DskipTests install
	mvn -o -q -f platform/integrations/spring/pom.xml -DskipTests install
	mvn -o -q -f platform/integrations/android/pom.xml -DskipTests install


platform-dependencies-install: platform-sample-preflight
	$(MAKE) platform-policy-install
	$(MAKE) platform-sample-install
	$(MAKE) platform-install-local-artifacts

platform-gradle-qualification:
	@grep -q '^distributionSha256Sum=' platform/integrations/android/sample-app/gradle/wrapper/gradle-wrapper.properties || { echo "Android Gradle wrapper distribution is not checksum-pinned" >&2; exit 1; }
	@test -s platform/integrations/android/sample-app/gradle/wrapper/gradle-wrapper.jar || { echo "Android Gradle wrapper jar is missing" >&2; exit 1; }
	@test -s platform/integrations/android/sample-app/gradle/verification-metadata.xml || { echo "Android Gradle dependency verification metadata is missing" >&2; exit 1; }

platform-js-install:
	npm ci --workspaces --include-workspace-root --ignore-scripts --no-audit --no-fund

platform-build:
	$(PLATFORM_CARGO) build --offline --manifest-path $(PLATFORM_MANIFEST) --locked --workspace
	$(MAKE) platform-sdk-check
	$(PLATFORM_CARGO) run --offline --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-release -- --check platform/release/registries.kvx

platform-build-all: platform-build platform-sample-dependencies-ready
	npm run build --workspaces --if-present
	cd platform/sdk/go && GOPROXY=off go build ./...
	cargo build --offline --locked --manifest-path platform/examples/marketplace/program/Cargo.toml --release
	mvn -o -q -f platform/sdk/jvm/pom.xml -DskipTests package
	mvn -o -q -f platform/integrations/spring/example/pom.xml -DskipTests package
	dotnet build platform/sdk/dotnet/LayerX.Sdk.csproj --no-restore --configuration Release --nologo
	swift build --disable-automatic-resolution --package-path platform/sdk/swift
	$(PLATFORM_PYTHON_ENV)/bin/python -m compileall -q agent/sdk/python platform/integrations/fastapi

platform-lint:
	$(PLATFORM_CARGO) clippy --offline --manifest-path $(PLATFORM_MANIFEST) --locked --workspace --all-targets -- -D warnings
	$(PLATFORM_CARGO) run --offline --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-workspace-gate -- platform/workspace.kvx .
	cargo clippy --offline --locked --manifest-path platform/examples/marketplace/program/Cargo.toml --all-targets -- -D warnings
	sh platform/tools/dependency-policy.sh
	cargo deny --disable-fetch --manifest-path $(PLATFORM_MANIFEST) check advisories bans sources

platform-test:
	$(PLATFORM_CARGO) test --offline --manifest-path $(PLATFORM_MANIFEST) --locked --workspace --features layerx-platform-cli/test-credential-store

platform-test-tooling: platform-test-cli-production-credential-refusal
	$(PLATFORM_CARGO) test --offline --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-cli --features test-credential-store
	$(PLATFORM_CARGO) test --offline --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-emulator
	$(PLATFORM_CARGO) test --offline --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-faucet
	$(PLATFORM_CARGO) test --offline --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-testnet
	sh -n platform/hosted/testnet/run-testnet.sh platform/hosted/testnet/reset-testnet.sh \
		platform/hosted/testnet/render-status.sh platform/hosted/testnet/tests/hosted-smoke.sh
	bash -n platform/cli/tests/install-journey.sh \
		platform/examples/agent-program-call/walkthrough.sh \
		platform/hosted/tests/topology-check.sh \
		platform/hosted/tests/beta-cluster.sh
	bash -n platform/cli/tests/clean-bootstrap.sh
	$(PLATFORM_CARGO) build --offline --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-cli
	bash platform/cli/tests/clean-bootstrap.sh

platform-test-cli-production-credential-refusal:
	bash platform/cli/tests/production-credential-refusal.sh

platform-test-registry:
	$(PLATFORM_CARGO) test --offline --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-registry

platform-hosted-smoke:
	@set -e; \
	if [ -r "$(PLATFORM_BETA_CLUSTER_ENV)" ]; then . "$(PLATFORM_BETA_CLUSTER_ENV)"; fi; \
	for variable in LAYERX_TESTNET_URL LAYERX_GATEWAY_URL LAYERX_FAUCET_URL LAYERX_TEST_AUTH_TOKEN_FILE \
		LAYERX_TEST_CA_FILE LAYERX_TEST_SOURCE_DID LAYERX_TEST_SOURCE_PUBLIC_KEY LAYERX_TEST_DESTINATION_DID \
		LAYERX_TEST_ASSET LAYERX_TEST_AMOUNT; do \
		eval "value=\$${$$variable:-}"; \
		if [ -z "$$value" ]; then echo "platform-hosted-smoke: missing input $$variable" >&2; exit 1; fi; \
	done; \
	sh platform/hosted/testnet/tests/hosted-smoke.sh

platform-hosted-topology-check:
	bash platform/hosted/tests/topology-check.sh

platform-beta-cluster-up:
	bash platform/hosted/tests/beta-cluster.sh up $(PLATFORM_BETA_CLUSTER_FLAGS)

platform-beta-cluster-down:
	bash platform/hosted/tests/beta-cluster.sh down

platform-test-agent-install:
	@test -n "$${LAYERX_BIN:-}"
	@test -n "$${LAYERX_GATEWAY_URL:-}"
	@test -n "$${LAYERX_NETWORK_ID:-}"
	@test -n "$${LAYERX_IDENTITY_TOKEN:-}"
	@test -n "$${LAYERX_SIGNING_SEED:-}"
	@test -n "$${LAYERX_SOURCE_ACCOUNT:-}"
	@test -n "$${LAYERX_PAYMENT_ASSET:-}"
	@test -n "$${LAYERX_PAYMENT_DESTINATION:-}"
	@test -n "$${LAYERX_MCP_SEQUENCE:-}"
	@test -n "$${LAYERX_A2A_SEQUENCE:-}"
	bash platform/cli/tests/install-journey.sh

platform-emulator-conformance:
	@test -n "$${LAYERX_TESTNET_URL:-}" && test -n "$${LAYERX_CONFORMANCE_ACTIVITIES:-}"
	sh platform/emulator/tests/conformance.sh \
		"$${LAYERX_EMULATOR_URL:-http://127.0.0.1:9402}" \
		"$$LAYERX_TESTNET_URL" "$${LAYERX_CONFORMANCE_ACTIVITIES}"

platform-sdk-check:
	python3 platform/sdk/generators/generate_portable.py --check $(CURDIR)
	python3 platform/sdk/generators/generate_jvm.py --check $(CURDIR)
	$(PLATFORM_CARGO) run --offline --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-sdkgen -- --check

platform-sdk-generate:
	cargo run --manifest-path agent/tools/sdk-gen/Cargo.toml --locked -- --write
	cargo run --manifest-path human/tools/api-gen/Cargo.toml --locked -- human/schema/human-api human/apps/web/src/api/generated
	python3 platform/sdk/generators/generate_portable.py $(CURDIR)
	python3 platform/sdk/generators/generate_jvm.py $(CURDIR)
	$(PLATFORM_CARGO) run --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-sdkgen -- --write

$(BUILD_DIR)/tools/generate_receipt_fixture: \
		platform/sdk/conformance/fixtures/generate_receipt_fixture.c \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) | programs-build
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< -Wl,--start-group \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) -Wl,--end-group \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -o $@

$(BUILD_DIR)/tools/generate_program_receipt_fixture: \
		platform/sdk/conformance/fixtures/generate_program_receipt_fixture.c \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) | programs-build
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< -Wl,--start-group \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) -Wl,--end-group \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -o $@

platform-receipt-fixture: $(BUILD_DIR)/tools/generate_receipt_fixture \
		$(BUILD_DIR)/tools/generate_program_receipt_fixture
	$(RUN_PREFIX) $(BUILD_DIR)/tools/generate_receipt_fixture \
		platform/sdk/conformance/fixtures/receipt-positive-v2.json
	$(RUN_PREFIX) $(BUILD_DIR)/tools/generate_program_receipt_fixture \
		platform/sdk/conformance/fixtures/receipt-programs-positive-v2.json \
		platform/sdk/conformance/fixtures/receipt-refusals-v2.json

platform-verify-sdks: platform-sdk-check platform-sample-dependencies-ready platform-js-install
	@python3 -c 'import pytest' || { echo "pytest is required for Python SDK conformance" >&2; exit 1; }
	npm run build --workspace @sidiora/layerx-sdk
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-sdk --tests
	./node_modules/.bin/tsc -p platform/sdk/conformance-runner/tsconfig.json
	cp platform/sdk/conformance-runner/package.json build/platform-sdk-conformance/package.json
	node --import ./platform/sdk/conformance-runner/register.mjs --test \
		build/platform-sdk-conformance/platform/sdk/conformance/secret-hygiene.test.js \
		build/platform-sdk-conformance/platform/sdk/conformance/streaming-resumability.test.js
	PYTHONPATH=agent/sdk/python python3 -m pytest --import-mode=importlib \
		platform/sdk/conformance/secret-hygiene.test.py \
		platform/sdk/conformance/streaming-resumability.test.py
	sh platform/sdk/conformance/run-go.sh $(CURDIR)
	sh platform/sdk/conformance/run-jvm.sh $(CURDIR)
	command -v swift >/dev/null
	swift build --disable-automatic-resolution --package-path platform/sdk/swift
	swift test --disable-automatic-resolution --package-path platform/sdk/swift
	command -v dotnet >/dev/null
	dotnet build platform/sdk/dotnet/LayerX.Sdk.csproj --no-restore --configuration Release --nologo
	dotnet test platform/sdk/dotnet/tests/LayerX.Sdk.Tests/LayerX.Sdk.Tests.csproj --no-restore --nologo

platform-test-sdks: platform-verify-sdks

platform-release-check:
	$(PLATFORM_CARGO) run --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-release -- --check platform/release/registries.kvx

platform-release-plan:
	$(PLATFORM_CARGO) run --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-release -- --plan platform/release/registries.kvx

platform-release-test:
	$(PLATFORM_CARGO) test --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-release

platform-test-middleware: platform-sample-dependencies-ready
	npm run build --workspace @sidiora/layerx-seller-middleware
	npm run build --workspace @sidiora/layerx-buyer-middleware
	npm run build --workspace @sidiora/layerx-merchant-middleware
	npm run build --workspace @sidiora/layerx-agent-middleware
	npm run build --workspace @sidiora/layerx-middleware-conformance
	npm run conformance --workspace @sidiora/layerx-middleware-conformance
	node --check platform/examples/merchant-shop/index.mjs
	node --check platform/examples/merchant-checkout/index.mjs
	node --check platform/examples/buyer-agent/index.mjs
	node --check platform/examples/paid-api/index.mjs
	node --check platform/examples/marketplace/index.mjs
	node --check platform/examples/support/runtime.mjs
	node --check platform/examples/support/merchant-app.mjs
	node --check platform/examples/agent-spend/index.mjs
	npm run build --workspace @sidiora/layerx-express
	npm run build --workspace @sidiora/layerx-next
	npm run build --workspace @sidiora/layerx-example-next
	npm run scan --workspace @sidiora/layerx-example-next
	PYTHONPATH=agent/sdk/python:platform/integrations/fastapi python3 -m compileall -q \
		platform/integrations/fastapi/layerx_fastapi platform/integrations/fastapi/examples
	npm run build --workspace @sidiora/layerx-agent-integrations
	mvn -o -q -f platform/integrations/android/pom.xml -DskipTests verify
	swift build --disable-automatic-resolution --package-path platform/integrations/ios

platform-test-reference-apps:
	node platform/examples/run-reference-apps.mjs --check

platform-test-mobile-artifacts: platform-test-android-artifact platform-test-ios-artifact

platform-prime-android-dependencies: platform-gradle-qualification
	gradle --no-daemon -p platform/integrations/android/sample-app layerxSecretScanRelease \
		-Playerx.serviceUrl=https://platform.example.com \
		-Playerx.sessionBrokerUrl=https://app-backend.example.com/layerx/session \
		-Playerx.eventKeyId=relay-2026 \
		-Playerx.eventPublicKey=d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a

platform-test-android-artifact: platform-sample-dependencies-ready platform-gradle-qualification
	gradle --offline --no-daemon -p platform/integrations/android/sample-app layerxSecretScanRelease \
		-Playerx.serviceUrl=https://platform.example.com \
		-Playerx.sessionBrokerUrl=https://app-backend.example.com/layerx/session \
		-Playerx.eventKeyId=relay-2026 \
		-Playerx.eventPublicKey=d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a

platform-test-ios-artifact: platform-sample-dependencies-ready
	command -v xcodegen >/dev/null || { echo "xcodegen is required for the iOS artifact" >&2; exit 1; }
	command -v xcodebuild >/dev/null || { echo "Xcode is required for the iOS artifact" >&2; exit 1; }
	cd platform/integrations/ios/sample-app && xcodegen generate
	xcodebuild -project platform/integrations/ios/sample-app/LayerXWallet.xcodeproj \
		-scheme LayerXWallet -configuration Release -sdk iphonesimulator \
		-derivedDataPath platform/integrations/ios/.derived CODE_SIGNING_ALLOWED=NO build
	swift run --disable-automatic-resolution --package-path platform/integrations/ios layerx-ios-secret-scan \
		platform/integrations/ios/.derived/Build/Products/Release-iphonesimulator/LayerXWallet.app

platform-real-agent-integration:
	@test -n "$${LAYERX_AGENT_FRAMEWORK:-}"
	@test -s "$${LAYERX_WEBHOOK_DELIVERY_PATH:-}"
	cd platform/integrations/agents && npm run integration:real

platform-real-ios-integration: platform-sample-dependencies-ready
	@test -s "$${LAYERX_SAMPLE_WEBHOOK_DELIVERY_PATH:-}"
	swift run --disable-automatic-resolution --package-path platform/integrations/ios layerx-ios-sample

platform-real-android-integration: platform-sample-dependencies-ready
	@test -s "$${LAYERX_SAMPLE_WEBHOOK_DELIVERY_PATH:-}"
	mvn -o -q -f platform/integrations/android/pom.xml -Psample test-compile exec:java

platform-test-docs: platform-test-reference-apps platform-sample-dependencies-ready
	sh platform/tools/docs-gate $(CURDIR)
	command -v node >/dev/null
	$(PLATFORM_PYTHON_ENV)/bin/python -m compileall -q platform/docs/samples/first-payment-python platform/docs/samples/paid-endpoint-fastapi
	cd platform/docs/samples/first-payment-go && GOPROXY=off go build ./...
	cargo build --offline --locked --manifest-path platform/docs/samples/verify-receipt-rust/Cargo.toml --release
	mvn -o -q -f platform/docs/samples/first-payment-jvm/pom.xml package
	mvn -o -q -f platform/docs/samples/paid-endpoint-spring/pom.xml package
	mvn -o -q -f platform/docs/samples/mobile-payment-android/pom.xml package
	swift build --disable-automatic-resolution --package-path platform/docs/samples/first-payment-swift
	swift build --disable-automatic-resolution --package-path platform/docs/samples/mobile-payment-ios
	dotnet build platform/docs/samples/first-payment-csharp/FirstPayment.csproj --no-restore --configuration Release --nologo
