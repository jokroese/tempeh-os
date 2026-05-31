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
firmware-env := "ESP_IDF_SYS_ROOT_CRATE=tempeh-firmware-esp32"
firmware-local:
    test -f crates/tempeh-firmware-esp32/firmware.local.toml || { echo "create crates/tempeh-firmware-esp32/firmware.local.toml first" >&2; exit 1; }
firmware-clean:
    rm -f sdkconfig sdkconfig.old
    rm -f crates/tempeh-firmware-esp32/sdkconfig crates/tempeh-firmware-esp32/sdkconfig.old
    rm -rf .embuild crates/tempeh-firmware-esp32/.embuild
    cargo clean
firmware-build: firmware-local
    cd crates/tempeh-firmware-esp32 && . ~/export-esp.sh && {{firmware-env}} cargo build --release
firmware-config-check:
    @echo "Generated firmware sdkconfig files:"
    @find target crates/tempeh-firmware-esp32/target -path '*/esp-idf-sys-*/out/sdkconfig' -print 2>/dev/null || true
    @echo
    @echo "Relevant firmware sdkconfig values:"
    @find target crates/tempeh-firmware-esp32/target -path '*/esp-idf-sys-*/out/sdkconfig' -print0 2>/dev/null \
      | xargs -0 rg "ESP_MAIN_TASK_STACK_SIZE|ESP_SYSTEM_EVENT_TASK_STACK_SIZE|FREERTOS_CHECK_STACKOVERFLOW|WATCHPOINT_END_OF_STACK|LOG_DEFAULT_LEVEL|ESP_CONSOLE_NONE|ESP_CONSOLE_USB_SERIAL_JTAG|ESP_CONSOLE_SECONDARY|ESP_CONSOLE_UART_NONE|CONSOLE_UART_NONE|PARTITION_TABLE_CUSTOM|ESPTOOLPY_FLASHSIZE" \
      || true
    @f=$$(find target -path '*/esp-idf-sys-*/out/sdkconfig' | head -n 1); \
      test -n "$$f" || { echo "ERROR: generated firmware sdkconfig not found" >&2; exit 1; }; \
      rg -q '^CONFIG_ESP_CONSOLE_USB_SERIAL_JTAG=y$$' "$$f" || { echo "ERROR: primary console is not USB Serial/JTAG" >&2; exit 1; }; \
      rg -q '^CONFIG_ESP_CONSOLE_SECONDARY_NONE=y$$' "$$f" || { echo "ERROR: secondary console is not disabled" >&2; exit 1; }; \
      rg -q '^CONFIG_ESP_MAIN_TASK_STACK_SIZE=65536$$' "$$f" || { echo "ERROR: main task stack size is not 65536" >&2; exit 1; }; \
      rg -q '^CONFIG_FREERTOS_CHECK_STACKOVERFLOW_CANARY=y$$' "$$f" || { echo "ERROR: stack overflow canary check is not enabled" >&2; exit 1; }; \
      rg -q '^CONFIG_PARTITION_TABLE_CUSTOM=y$$' "$$f" || { echo "ERROR: custom partition table is not enabled" >&2; exit 1; }; \
      rg -q '^CONFIG_ESPTOOLPY_FLASHSIZE="4MB"$$' "$$f" || { echo "ERROR: flash size is not 4MB" >&2; exit 1; }
firmware-flash port: firmware-local
    cd crates/tempeh-firmware-esp32 && . ~/export-esp.sh && {{firmware-env}} ESPFLASH_PORT={{port}} cargo run --release
firmware-rebuild: firmware-clean firmware-build firmware-config-check
