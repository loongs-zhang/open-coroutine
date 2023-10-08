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

# test open-coroutine-core mod
cd "${PROJECT_DIR}"/open-coroutine-core
"${CARGO}" test --target "${TARGET}" --no-default-features
"${CARGO}" test --target "${TARGET}" --no-default-features --release

if [ "${TARGET}" != "riscv64gc-unknown-linux-gnu" ]; then
    "${CARGO}" test --target "${TARGET}" --no-default-features --features preemptive-schedule
fi

"${CARGO}" test --target "${TARGET}" --no-default-features --features preemptive-schedule --release
"${CARGO}" run --example preemptive --release --features preemptive-schedule

# test open-coroutine-iouring mod
if [ "${TARGET}" = "x86_64-unknown-linux-gnu" ]; then
    cd "${PROJECT_DIR}"/open-coroutine-iouring
    "${CARGO}" test --target "${TARGET}" --no-default-features
    "${CARGO}" test --target "${TARGET}" --no-default-features --release
fi

# test open-coroutine mod
cd "${PROJECT_DIR}"/open-coroutine
"${CARGO}" test --target "${TARGET}" --no-default-features
"${CARGO}" test --target "${TARGET}" --no-default-features --release

# test examples
cd "${PROJECT_DIR}"/examples
"${CARGO}" run --example sleep_not_co --release
"${CARGO}" run --example sleep_co --release
"${CARGO}" run --example socket_not_co --release
"${CARGO}" run --example socket_co_server --release
"${CARGO}" run --example socket_co_client --release
"${CARGO}" run --example socket_co --release
