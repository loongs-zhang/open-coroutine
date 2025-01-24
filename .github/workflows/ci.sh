#!/usr/bin/env sh

set -ex

CARGO=cargo
if [ "${CROSS}" = "1" ]; then
    export CARGO_NET_RETRY=5
    export CARGO_NET_TIMEOUT=10

    cargo install cross --git https://github.com/cross-rs/cross --rev 4090beca3cfffa44371a5bba524de3a578aa46c3
    CARGO=cross
fi

# If a test crashes, we want to know which one it was.
export RUST_TEST_THREADS=1
export RUST_BACKTRACE=1

# todo remove this
if [ "${OS}" = "windows-latest" ]; then
    cd "${PROJECT_DIR}"/open-coroutine
    "${CARGO}" test --target "${TARGET}" --no-default-features --features iocp,ci
    "${CARGO}" test --target "${TARGET}" --no-default-features --features iocp,ci --release
fi

# test open-coroutine-core mod
cd "${PROJECT_DIR}"/core
"${CARGO}" test --target "${TARGET}" --features ci
"${CARGO}" test --target "${TARGET}" --features ci --release

# test open-coroutine
cd "${PROJECT_DIR}"/open-coroutine
"${CARGO}" test --target "${TARGET}" --features ci
"${CARGO}" test --target "${TARGET}" --features ci --release

# test io_uring
if [ "${TARGET}" = "x86_64-unknown-linux-gnu" ]; then
    cd "${PROJECT_DIR}"/core
    "${CARGO}" test --target "${TARGET}" --no-default-features --features io_uring,ci
    "${CARGO}" test --target "${TARGET}" --no-default-features --features io_uring,ci --release
    cd "${PROJECT_DIR}"/open-coroutine
    "${CARGO}" test --target "${TARGET}" --no-default-features --features io_uring,ci
    "${CARGO}" test --target "${TARGET}" --no-default-features --features io_uring,ci --release
fi

# test IOCP
if [ "${OS}" = "windows-latest" ]; then
    cd "${PROJECT_DIR}"/core
    "${CARGO}" test --target "${TARGET}" --no-default-features --features iocp,ci
    "${CARGO}" test --target "${TARGET}" --no-default-features --features iocp,ci --release
    cd "${PROJECT_DIR}"/open-coroutine
    "${CARGO}" test --target "${TARGET}" --no-default-features --features iocp,ci
    "${CARGO}" test --target "${TARGET}" --no-default-features --features iocp,ci --release
fi
