# compid developer tasks - thin wrappers over cargo and python.
# The justfile mirrors these; use whichever you have installed.

.PHONY: help build test fmt check lint corpus train functions-model mixed-demo smoke dist man clean

help:
	@echo "compid make targets:"
	@echo "  build          - release build of the CLI"
	@echo "  test           - run the test suite"
	@echo "  fmt            - apply rustfmt"
	@echo "  check          - rustfmt check + clippy (what CI runs)"
	@echo "  corpus         - rebuild the seed corpus into dataset/binaries"
	@echo "  train          - retrain model/model.json (needs .venv, see training/requirements.txt)"
	@echo "  functions-model - retrain model/functions.json from a functions JSONL (FUNCTIONS_JSONL=path)"
	@echo "  mixed-demo     - the mixed-toolchain function-level acceptance script"
	@echo "  smoke          - quick end-to-end sanity pass"
	@echo "  dist           - release tarball + completions + sha256 into dist/ (scripts/dist.sh)"
	@echo "  man            - render the roff man page (docs/compid.1) to stdout"
	@echo "  clean          - remove build artifacts"

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

FUNCTIONS_JSONL ?= dataset/features/functions.jsonl
functions-model:
	.venv/bin/python training/train_functions.py --features-in "$(FUNCTIONS_JSONL)"

mixed-demo: build
	./scripts/mixed_demo.sh

smoke: build
	./target/release/compid dataset-info
	./target/release/compid classify "$$(find dataset/binaries -type f | head -n 1)"

dist:
	./scripts/dist.sh

man:
	groff -man -Tutf8 docs/compid.1

clean:
	cargo clean
