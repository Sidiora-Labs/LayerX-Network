PLATFORM_CARGO ?= cargo
PLATFORM_MANIFEST := platform/Cargo.toml

.PHONY: platform-build platform-lint platform-test platform-sdk-check \
	platform-sdk-generate platform-test-sdks platform-release-check platform-release-plan \
	platform-test-tooling platform-emulator-conformance platform-test-middleware \
	platform-test-reference-apps platform-test-mobile-artifacts \
	platform-test-android-artifact platform-test-ios-artifact \
	platform-real-agent-integration \
	platform-real-ios-integration platform-real-android-integration \
	platform-test-docs platform-hosted-smoke platform-test-agent-install

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
	$(PLATFORM_CARGO) test --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-cli
	$(PLATFORM_CARGO) check --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-emulator
	$(PLATFORM_CARGO) test --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-faucet
	$(PLATFORM_CARGO) test --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-testnet
	sh -n platform/hosted/testnet/run-testnet.sh platform/hosted/testnet/reset-testnet.sh \
		platform/hosted/testnet/render-status.sh platform/hosted/testnet/tests/hosted-smoke.sh
	bash -n platform/cli/tests/install-journey.sh
	@if test -n "$${LAYERX_TESTNET_URL:-}" && test -n "$${LAYERX_CONFORMANCE_ACTIVITIES:-}"; then \
		sh platform/emulator/tests/conformance.sh \
			"$${LAYERX_EMULATOR_URL:-http://127.0.0.1:9402}" \
			"$$LAYERX_TESTNET_URL" "$${LAYERX_CONFORMANCE_ACTIVITIES}"; \
	else \
		echo "emulator/testnet differential skipped: set LAYERX_TESTNET_URL and LAYERX_CONFORMANCE_ACTIVITIES"; \
	fi

platform-hosted-smoke:
	@test -n "$${LAYERX_TESTNET_URL:-}"
	@test -n "$${LAYERX_GATEWAY_URL:-}"
	@test -n "$${LAYERX_FAUCET_URL:-}"
	@test -n "$${LAYERX_TEST_AUTH_TOKEN_FILE:-}"
	@test -n "$${LAYERX_TEST_CA_FILE:-}"
	@test -n "$${LAYERX_TEST_SOURCE_DID:-}"
	@test -n "$${LAYERX_TEST_SOURCE_PUBLIC_KEY:-}"
	@test -n "$${LAYERX_TEST_DESTINATION_DID:-}"
	@test -n "$${LAYERX_TEST_ASSET:-}"
	@test -n "$${LAYERX_TEST_AMOUNT:-}"
	sh platform/hosted/testnet/tests/hosted-smoke.sh

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
	$(PLATFORM_CARGO) run --manifest-path $(PLATFORM_MANIFEST) --locked -p layerx-platform-sdkgen -- --check

platform-sdk-generate:
	cargo run --manifest-path agent/tools/sdk-gen/Cargo.toml --locked -- --write
	cargo run --manifest-path human/tools/api-gen/Cargo.toml --locked -- human/schema/human-api human/apps/web/src/api/generated
	python3 platform/sdk/generators/generate_portable.py $(CURDIR)
	python3 platform/sdk/generators/generate_jvm.py $(CURDIR)
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
	cd platform/middleware/merchant && npm install --no-audit --no-fund && npm run build
	cd platform/middleware/agent && npm install --no-audit --no-fund && npm run build
	cd platform/middleware/conformance && npm install --no-audit --no-fund && npm run build && npm run conformance
	node --check platform/examples/merchant-shop/index.mjs
	node --check platform/examples/merchant-checkout/index.mjs
	node --check platform/examples/buyer-agent/index.mjs
	node --check platform/examples/paid-api/index.mjs
	node --check platform/examples/marketplace/index.mjs
	node --check platform/examples/support/runtime.mjs
	node --check platform/examples/support/merchant-app.mjs
	node --check platform/examples/agent-spend/index.mjs
	cd platform/integrations/express && npm install --no-audit --no-fund && npm run build
	cd platform/integrations/next && npm install --no-audit --no-fund && npm run build
	npm install --no-audit --no-fund --workspace @sidiora/layerx-example-next
	npm run build --workspace @sidiora/layerx-example-next
	npm run scan --workspace @sidiora/layerx-example-next
	PYTHONPATH=agent/sdk/python:platform/integrations/fastapi python3 -m compileall -q \
		platform/integrations/fastapi/layerx_fastapi platform/integrations/fastapi/examples
	mvn -q -f platform/sdk/jvm/pom.xml -DskipTests install
	mvn -q -f platform/integrations/spring/pom.xml -DskipTests install
	cd platform/integrations/agents && npm install --no-audit --no-fund && npm run build
	mvn -q -f platform/integrations/android/pom.xml -DskipTests verify
	command -v swift >/dev/null
	swift build --package-path platform/integrations/ios

platform-test-reference-apps:
	node platform/examples/run-reference-apps.mjs --check

platform-test-mobile-artifacts: platform-test-android-artifact platform-test-ios-artifact

platform-test-android-artifact:
	mvn -q -f platform/sdk/jvm/pom.xml -DskipTests install
	mvn -q -f platform/integrations/android/pom.xml -DskipTests install
	gradle --no-daemon -p platform/integrations/android/sample-app layerxSecretScanRelease \
		-Playerx.serviceUrl=https://platform.example.com \
		-Playerx.sessionBrokerUrl=https://app-backend.example.com/layerx/session \
		-Playerx.eventKeyId=relay-2026 \
		-Playerx.eventPublicKey=d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a

platform-test-ios-artifact:
	command -v xcodegen >/dev/null
	cd platform/integrations/ios/sample-app && xcodegen generate
	xcodebuild -project platform/integrations/ios/sample-app/LayerXWallet.xcodeproj \
		-scheme LayerXWallet -configuration Release -sdk iphonesimulator \
		-derivedDataPath platform/integrations/ios/.derived CODE_SIGNING_ALLOWED=NO build
	swift run --package-path platform/integrations/ios layerx-ios-secret-scan \
		platform/integrations/ios/.derived/Build/Products/Release-iphonesimulator/LayerXWallet.app

platform-real-agent-integration:
	@test -n "$${LAYERX_AGENT_FRAMEWORK:-}"
	@test -s "$${LAYERX_WEBHOOK_DELIVERY_PATH:-}"
	cd platform/integrations/agents && npm run integration:real

platform-real-ios-integration:
	@test -s "$${LAYERX_SAMPLE_WEBHOOK_DELIVERY_PATH:-}"
	swift run --package-path platform/integrations/ios layerx-ios-sample

platform-real-android-integration:
	@test -s "$${LAYERX_SAMPLE_WEBHOOK_DELIVERY_PATH:-}"
	mvn -q -f platform/sdk/jvm/pom.xml -DskipTests install
	mvn -q -f platform/integrations/android/pom.xml -DskipTests install
	mvn -q -f platform/integrations/android/pom.xml -Psample test-compile exec:java

platform-test-docs: platform-test-reference-apps
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
