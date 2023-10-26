#!/usr/bin/env sh

set -ex

CARGO=cargo
if [ "${CROSS}" = "1" ]; then
    export CARGO_NET_RETRY=5
    export CARGO_NET_TIMEOUT=10

    cargo install cross
    CARGO=cross
fi

# If a test crashes, we want to know which one it was.
export RUST_TEST_THREADS=1
export RUST_BACKTRACE=1

# test open-coroutine-timer mod
cd "${PROJECT_DIR}"/open-coroutine-timer
"${CARGO}" test --target "${TARGET}" --no-default-features
"${CARGO}" test --target "${TARGET}" --no-default-features --release

# test open-coroutine-queue mod
cd "${PROJECT_DIR}"/open-coroutine-queue
"${CARGO}" test --target "${TARGET}" --no-default-features
"${CARGO}" test --target "${TARGET}" --no-default-features --release
