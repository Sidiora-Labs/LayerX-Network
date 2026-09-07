FROM debian:bookworm

ARG DEBIAN_FRONTEND=noninteractive
ARG GO_VERSION=1.25.6
ARG GO_SHA256=f022b6aad78e362bcba9b0b94d09ad58c5a70c6ba3b7582905fababf5fe0181a
ARG NODE_VERSION=24.11.0
ARG NODE_SHA256=46da9a098973ab7ba4fca76945581ecb2eaf468de347173897044382f10e0a0a
ARG RUST_VERSION=1.91.1
ARG FOUNDRY_VERSION=v1.7.1
ARG FOUNDRY_SHA256=cf7e688ed0c4c48adffca788b496076e31060b67ac5afe1e43dbb5499c20c88b
ARG KIND_VERSION=v0.30.0
ARG KIND_SHA256=517ab7fc89ddeed5fa65abf71530d90648d9638ef0c4cde22c2c11f8097b8889
ARG KUBECTL_VERSION=v1.34.1
ARG KUBECTL_SHA256=7721f265e18709862655affba5343e85e1980639395d5754473dafaadcaa69e3
ARG USERNAME=vscode
ARG USER_UID=1000
ARG USER_GID=1000

ENV LANG=C.UTF-8 \
    LC_ALL=C.UTF-8 \
    TZ=UTC \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    GOPATH=/home/${USERNAME}/go \
    GOROOT=/usr/local/go \
    CC=clang \
    CXX=clang++ \
    PATH=/usr/local/cargo/bin:/usr/local/go/bin:/usr/local/node/bin:/home/${USERNAME}/go/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        ca-certificates \
        curl \
        wget \
        gnupg \
        git \
        make \
        cmake \
        pkg-config \
        libssl-dev \
        libsqlite3-dev \
        python3 \
        python3-venv \
        python3-pip \
        python3-yaml \
        build-essential \
        jq \
        sudo \
        xz-utils \
        unzip \
        file \
        ripgrep \
    && wget -qO- https://apt.llvm.org/llvm-snapshot.gpg.key \
        | gpg --dearmor -o /usr/share/keyrings/llvm-archive-keyring.gpg \
    && echo "deb [signed-by=/usr/share/keyrings/llvm-archive-keyring.gpg] https://apt.llvm.org/bookworm llvm-toolchain-bookworm-18 main" \
        > /etc/apt/sources.list.d/llvm.list \
    && apt-get update \
    && apt-get install --yes --no-install-recommends \
        clang-18 \
        lld-18 \
        llvm-18 \
        libclang-rt-18-dev \
    && update-alternatives --install /usr/bin/clang clang /usr/bin/clang-18 100 \
    && update-alternatives --install /usr/bin/clang++ clang++ /usr/bin/clang++-18 100 \
    && update-alternatives --install /usr/bin/cc cc /usr/bin/clang-18 50 \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL "https://go.dev/dl/go${GO_VERSION}.linux-amd64.tar.gz" -o /tmp/go.tar.gz \
    && echo "${GO_SHA256}  /tmp/go.tar.gz" | sha256sum -c - \
    && tar -C /usr/local -xzf /tmp/go.tar.gz \
    && rm /tmp/go.tar.gz

RUN curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" -o /tmp/node.tar.xz \
    && echo "${NODE_SHA256}  /tmp/node.tar.xz" | sha256sum -c - \
    && mkdir -p /usr/local/node \
    && tar -xJf /tmp/node.tar.xz -C /usr/local/node --strip-components=1 \
    && rm /tmp/node.tar.xz

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile minimal --default-toolchain "${RUST_VERSION}" --component rustfmt,clippy \
    && /usr/local/cargo/bin/rustup target add --toolchain "${RUST_VERSION}" wasm32-unknown-unknown \
    && chmod -R a+rX /usr/local/rustup /usr/local/cargo

RUN curl -fsSL "https://github.com/foundry-rs/foundry/releases/download/${FOUNDRY_VERSION}/foundry_${FOUNDRY_VERSION}_linux_amd64.tar.gz" -o /tmp/foundry.tar.gz \
    && echo "${FOUNDRY_SHA256}  /tmp/foundry.tar.gz" | sha256sum -c - \
    && tar -C /usr/local/bin -xzf /tmp/foundry.tar.gz forge cast anvil chisel \
    && rm /tmp/foundry.tar.gz \
    && chmod 0755 /usr/local/bin/forge /usr/local/bin/cast /usr/local/bin/anvil /usr/local/bin/chisel

RUN curl -fsSL "https://github.com/kubernetes-sigs/kind/releases/download/${KIND_VERSION}/kind-linux-amd64" -o /usr/local/bin/kind \
    && echo "${KIND_SHA256}  /usr/local/bin/kind" | sha256sum -c - \
    && chmod 0755 /usr/local/bin/kind \
    && curl -fsSL "https://dl.k8s.io/release/${KUBECTL_VERSION}/bin/linux/amd64/kubectl" -o /usr/local/bin/kubectl \
    && echo "${KUBECTL_SHA256}  /usr/local/bin/kubectl" | sha256sum -c - \
    && chmod 0755 /usr/local/bin/kubectl

RUN groupadd --gid "${USER_GID}" "${USERNAME}" \
    && useradd --uid "${USER_UID}" --gid "${USER_GID}" -m -s /bin/bash "${USERNAME}" \
    && echo "${USERNAME} ALL=(root) NOPASSWD:ALL" > "/etc/sudoers.d/${USERNAME}" \
    && chmod 0440 "/etc/sudoers.d/${USERNAME}" \
    && mkdir -p "/home/${USERNAME}/go" \
    && chown -R "${USERNAME}:${USERNAME}" "/home/${USERNAME}"

USER ${USERNAME}
WORKDIR /workspaces/LayerX-Protocol
