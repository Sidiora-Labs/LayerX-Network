all: simd-env

simd-env: simd-rmi
	docker build --tag cosmossdk/simd -f simd-env/Dockerfile \
		$(abspath ../../..)

simd-dlv: simd-rmi
	docker build --tag cosmossdk/simd -f simd-dlv/Dockerfile \
		$(abspath ../../..)

simd-rmi:
	docker rmi cosmossdk/simd 2>/dev/null; true

.PHONY: all simd-env simd-dlv simd-rmi
