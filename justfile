# compid developer tasks - thin wrappers over cargo and python.
# The Makefile mirrors these; use whichever you have installed.

default:
    @just --list

build:
    cargo build --release

test:
    cargo test

fmt:
    cargo fmt --all

check:
    cargo fmt --all -- --check
    cargo clippy --all-targets

corpus:
    python3 corpus/build_corpus.py

train:
    .venv/bin/python training/train.py

smoke: build
    ./target/release/compid dataset-info
    ./target/release/compid classify "$(find dataset/binaries -type f | head -n 1)"

clean:
    cargo clean
