.PHONY: build build-linux test run clean lint check docker desktop-build desktop-test desktop-run

# === Go Web Hosting (Render / Railway / Fly.io / Docker) ===
build:
	CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o target/salehunter_server.exe ./cmd/server

build-linux:
	CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -trimpath -ldflags="-s -w" -o target/salehunter_bot_linux ./cmd/server

check:
	CGO_ENABLED=0 go vet ./...

test:
	CGO_ENABLED=0 go test -v ./...

run:
	CGO_ENABLED=0 go run ./cmd/server

docker:
	docker build -t salehunter-bot .

lint:
	CGO_ENABLED=0 go vet ./...

clean:
	del /f /q target\salehunter_server.exe target\salehunter_bot_linux *.db *.db-journal *.db-wal *.db-shm *.bak 2>nul || exit 0

# === Rust Windows Desktop App (PC Local GUI) ===
desktop-build:
	cargo build --release

desktop-test:
	cargo test --bin salehunter_bot

desktop-run:
	cargo run --release
