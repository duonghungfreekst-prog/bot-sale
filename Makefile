.PHONY: build test run clean lint check docker

build:
	cargo build --release

check:
	cargo check

test:
	cargo test --bin salehunter_bot

run:
	cargo run --release

docker:
	docker build -t salehunter-bot .

clean:
	cargo clean
	del /f /q *.db *.db-journal *.db-wal *.db-shm 2>nul || exit 0

lint:
	cargo clippy --bin salehunter_bot -- -D warnings -W clippy::all
