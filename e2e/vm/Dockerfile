# The Linux environment of every local build and gate (e2e/vm.sh builds it as practical-e2e-vm:<hash of this file +
# the build args>; never pushed): Rust (the channel of build/rust-toolchain.toml, ARG TOOLCHAIN), Node, Playwright's
# Chromium + its OS packages (ARG PLAYWRIGHT = e2e/package.json's @playwright/test), mbx (shared compiler cache),
# watchexec (`mise run dev`), sqlite3 (bench), cwebp (readme-shots). glibc, not the image build's musl/alpine:
# Playwright ships no Chromium for musl. Downloads are pinned by SHA-256.
# renovate: datasource=docker packageName=rust
FROM rust:1.99-bookworm@sha256:7085845219c18e7c5b1f34ed9ce89b84f822f2e3d54a02774cedc996ac0f304f
ARG TARGETARCH
ARG TOOLCHAIN
ARG PLAYWRIGHT
# e2e/cargo.sh exports RUSTUP_TOOLCHAIN=<channel>; rustup treats the image's own `1.99.1` as another toolchain and
# would install the channel on first use in every new container
RUN test -n "$TOOLCHAIN" && rustup toolchain install "$TOOLCHAIN" --profile minimal && rustup default "$TOOLCHAIN"
# Node = the host's mise version (24.7.0, 2026-11-03); hashes from nodejs.org/dist/v24.7.0/SHASUMS256.txt
RUN set -eu; case "$TARGETARCH" in \
      arm64) a=arm64 s=3c238d671b2cc2e1c62b8b068c346b9830708f90df1e4a37305f4a5c2bb230b8 ;; \
      amd64) a=x64 s=2fb405154d017f04d21b3d2273cc1cdfa824cfeffbd4225976454d06d5e381a4 ;; *) exit 1 ;; esac; \
    curl -fsSLo /tmp/node.tar.xz "https://nodejs.org/dist/v24.7.0/node-v24.7.0-linux-$a.tar.xz"; \
    echo "$s  /tmp/node.tar.xz" | sha256sum -c - >/dev/null; \
    tar -xJf /tmp/node.tar.xz -C /usr/local --strip-components=1 --exclude='*.md' --exclude=LICENSE; rm /tmp/node.tar.xz
# mbx 1.29.0: the pins of ai/skills building-rust-projects/scripts/rust-dev-container.sh (checked there against the
# release's sigstore attestation, 2026-10-30); watchexec 2.8.0: release asset digests (GitHub API = the .sha256 files); its static musl build (the gnu one needs glibc 2.39, bookworm has 2.36)
RUN set -eu; case "$TARGETARCH" in \
      arm64) m=aarch64 ms=eeee519621082424fae16799949514b6000ba733a2d809c709a3d3ae4a0b18f7 ws=3961878b21e3c1729279322f4007dd5bbf0032110fbcb5e7b983946a9ceb3bf6 ;; \
      amd64) m=x86_64 ms=b713e1a3b8e7a8696b7e365ef222a5b4bb45b18ad6c30b6f86972194294c31ae ws=35ea667cbe4bc96c23ba2a1f46c94c46bc385b67165e3bf621c0e7e4959aa99b ;; *) exit 1 ;; esac; \
    curl -fsSLo /tmp/mbx.tgz "https://github.com/jdx/mr-boxington/releases/download/v1.29.0/mbx-$m-unknown-linux-gnu.tar.gz"; \
    echo "$ms  /tmp/mbx.tgz" | sha256sum -c - >/dev/null; \
    mkdir /tmp/mbx && tar -xzf /tmp/mbx.tgz -C /tmp/mbx && install -m 0755 "$(find /tmp/mbx -type f -name mbx)" /usr/local/bin/mbx; \
    curl -fsSLo /tmp/wx.txz "https://github.com/watchexec/watchexec/releases/download/v2.8.0/watchexec-2.8.0-$m-unknown-linux-musl.tar.xz"; \
    echo "$ws  /tmp/wx.txz" | sha256sum -c - >/dev/null; \
    tar -xJf /tmp/wx.txz -C /tmp && install -m 0755 "/tmp/watchexec-2.8.0-$m-unknown-linux-musl/watchexec" /usr/local/bin/watchexec; \
    rm -rf /tmp/mbx /tmp/mbx.tgz /tmp/wx.txz /tmp/watchexec-*
ENV PLAYWRIGHT_BROWSERS_PATH=/ms-playwright
# fonts-inter: "Inter" is the first entry of base.css' --font stack a Linux has; without it Chromium falls to DejaVu
# Sans, wide enough that 2 phone-width functional tests overflowed 390 px by 2.5 px (2026-11-03)
RUN test -n "$PLAYWRIGHT" && apt-get update -qq && apt-get install -y -qq --no-install-recommends sqlite3 webp fonts-inter >/dev/null \
 && npx -y "playwright@$PLAYWRIGHT" install --with-deps chromium \
 && rm -rf /var/lib/apt/lists/* /root/.npm
# the checkout is bind-mounted from the host (owner uid 501 ≠ root): git would refuse it as "dubious ownership"
RUN git config --system --add safe.directory '*'
