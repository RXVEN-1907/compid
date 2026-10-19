# compid developer tasks - thin wrappers over cargo and python.
# The justfile mirrors these; use whichever you have installed.

.PHONY: help build test fmt check lint corpus train smoke clean

help:
	@echo "compid make targets:"
	@echo "  build   - release build of the CLI"
	@echo "  test    - run the test suite"
	@echo "  fmt     - apply rustfmt"
	@echo "  check   - rustfmt check + clippy (what CI runs)"
	@echo "  corpus  - rebuild the seed corpus into dataset/binaries"
	@echo "  train   - retrain model/model.json (needs .venv, see training/requirements.txt)"
	@echo "  smoke   - quick end-to-end sanity pass"
	@echo "  clean   - remove build artifacts"

build:
	cargo build --release

test:
	cargo test

fmt:
	cargo fmt --all

check:
	cargo fmt --all -- --check
	cargo clippy --all-targets

lint: check

corpus:
	python3 corpus/build_corpus.py

train:
	.venv/bin/python training/train.py

smoke: build
	./target/release/compid dataset-info
	./target/release/compid classify "$$(find dataset/binaries -type f | head -n 1)"

clean:
	cargo clean
