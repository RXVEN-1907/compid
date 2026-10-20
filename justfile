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

functions-model features_in="dataset/features/functions.jsonl":
    .venv/bin/python training/train_functions.py --features-in "{{features_in}}"

mixed-demo: build
    ./scripts/mixed_demo.sh

smoke: build
    ./target/release/compid dataset-info
    ./target/release/compid classify "$(find dataset/binaries -type f | head -n 1)"

dist:
    ./scripts/dist.sh

man:
    groff -man -Tutf8 docs/compid.1

clean:
    cargo clean
