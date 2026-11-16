.PHONY: build test run clean lint check

build:
	cargo build --release

check:
	cargo check

test:
	cargo test

run:
	cargo run --release

clean:
	cargo clean
	del /f /q *.db *.db-journal *.db-wal *.db-shm 2>nul || exit 0

lint:
	cargo clippy -- -D warnings
