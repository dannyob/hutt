PREFIX ?= ~/.local

.PHONY: build install check test clippy clean

build:
	cargo build --release

install: build
	cargo install --path . --root $(PREFIX)

check:
	cargo check

test:
	cargo test

clippy:
	cargo clippy -- -W clippy::all

clean:
	cargo clean
