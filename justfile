set shell := ["bash", "-cu"]
default:
    @just --list
fmt:
    cargo fmt --check
test:
    cargo test
host *args:
    cargo run -p tempeh-host -- {{args}}
help:
    cargo run -p tempeh-host -- help
smoke-stdin:
    printf "temp,box_air,22.4\ntemp,product,23.1\n" \
      | cargo run -p tempeh-host -- thermometer-test - \
      | grep -q "22.400,23.100"
firmware-deps:
    if cargo tree -p tempeh-firmware-esp32 | rg "ureq|rustls|ring"; then \
      echo "firmware dependency graph must not include host HTTP/TLS crates" >&2; \
      exit 1; \
    fi
check: fmt test help smoke-stdin firmware-deps
firmware-build:
    test -f crates/tempeh-firmware-esp32/firmware.local.toml || { echo "create crates/tempeh-firmware-esp32/firmware.local.toml first" >&2; exit 1; }
    cd crates/tempeh-firmware-esp32 && . ~/export-esp.sh && cargo build --release
firmware-flash port:
    test -f crates/tempeh-firmware-esp32/firmware.local.toml || { echo "create crates/tempeh-firmware-esp32/firmware.local.toml first" >&2; exit 1; }
    cd crates/tempeh-firmware-esp32 && . ~/export-esp.sh && ESPFLASH_PORT={{port}} cargo run --release
