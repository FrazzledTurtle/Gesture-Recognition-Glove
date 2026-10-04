# Gesture Recognition Glove — Rust rewrite (no_std)

A `no_std`, bare-metal port of the original Arduino/PlatformIO (C++) firmware
— preserved in the [v0.1 release](https://github.com/FrazzledTurtle/Gesture-Recognition-Glove/releases/tag/v0.1),
no longer in the current tree — to Rust, targeting the same ESP32 DOIT DevKit V1 board. Built on `esp-hal`
(Espressif's bare-metal HAL) + `esp-radio`/`esp-rtos` for Wi-Fi + `embassy`
for the async runtime and TCP/IP stack — not `esp-idf`, so there's no C
toolchain or ESP-IDF install involved, only the Espressif Rust/LLVM fork.

**This has been built and clippy-checked against real crates.io dependency
resolution** (`cargo build --release` succeeds, producing a Xtensa ELF). It
has not been flashed to real hardware in this environment — I don't have a
board attached here, so the usual first-boot surprises (timing, wiring,
register quirks) are still worth watching for.

Same pinout as the original:

| Signal       | GPIO |
|--------------|------|
| LED          | 2    |
| Flex sensor 1| 34   |
| Flex sensor 2| 35   |
| Flex sensor 3| 33   |
| Flex sensor 4| 32   |
| Flex sensor 5| 39   |
| I2C SDA (MPU6050) | 21 |
| I2C SCL (MPU6050) | 22 |

All five flex sensor pins are ADC1 channels, same as the original — ADC2
can't be read while Wi-Fi is active on the ESP32 (esp-hal enforces this with
a panic if you try).

## Architecture notes

- **MPU6050**: the published `mpu6050` crate (0.1.6 on crates.io) still
  targets embedded-hal 0.2's blocking traits; `esp-hal`'s I2C driver only
  implements embedded-hal 1.0. Rather than pull in a shim, `src/main.rs` talks
  to the chip's registers directly (wake it via `PWR_MGMT_1`, read
  `ACCEL_XOUT_H`/`GYRO_XOUT_H`) — about 30 lines, fully under our control.
- **Concurrency**: there's no OS and no thread pool — four async loops
  (Wi-Fi connection management, the embassy-net driver poll, sensor
  reading, and the HTTP server) are run concurrently with
  `embassy_futures::join::join4` inside one `#[esp_hal::main] async fn
  main`, rather than `embassy_executor` tasks. This sidesteps having to name
  the concrete, chip-specific GPIO pin types that the `#[embassy_executor::task]`
  macro would require (it can't take generic parameters); plain async
  functions can.
- **HTTP server**: no_std has no mature off-the-shelf async HTTP *server*
  crate as battle-tested as `esp-idf-svc`'s (there's `reqwless` for HTTP
  *client* use, which the upstream esp-hal Wi-Fi examples use, but that's the
  wrong direction). `http_loop` in `src/main.rs` is a minimal hand-rolled
  responder directly over `embassy_net::tcp::TcpSocket`: it reads the request
  line, checks for `GET /read_gesture`, and writes back a fixed HTTP/1.1
  header plus body with `Connection: close` (no `Content-Length` needed since
  the socket close marks the end, which both browsers and the page's own
  `XMLHttpRequest` handle fine).
- **Gesture state**: shared between the sensor loop and the HTTP server via
  an `embassy_sync::mutex::Mutex<CriticalSectionRawMutex, &'static str>` —
  gesture names are all `'static` string literals, so no heap `String` is
  needed for that part. A small heap (`esp_alloc`, 64 KiB) is still set up
  because `esp-radio`'s Wi-Fi driver needs one internally.

## One behavioral note carried over from the std version

The original `Calibration()` compared raw ADC counts (0..4095) against
`R_Straight`/`R_Bend`, which were initialized to resistance values (12300.0 /
29000.0 ohms). A raw count can never reach into the tens of thousands, so
`R_Bend` could never update, and neither variable was ever read anywhere else
in the firmware — gesture detection only ever used the fixed voltage-divider
formula and the hardcoded thresholds. The only real effect of that function
was the 15s pause it gave the user to put the glove on before readings
start. This rewrite reproduces that pause (`calibration_delay()`) without
resurrecting the dead bookkeeping.

## Setup

You need the Espressif Rust toolchain (for the Xtensa ESP32 target), installed
via [`espup`](https://github.com/esp-rs/espup):

```sh
cargo install espup
espup install --targets esp32
. $HOME/export-esp.sh   # re-run in every new shell, or source it from your rc file
```

`rust-toolchain.toml` pins this project to the `esp` toolchain channel
automatically; `export-esp.sh` additionally puts the Xtensa GCC linker and
clang on `PATH`, which rustup's toolchain file can't do by itself.

Install `espflash` for flashing (not needed just to build/check):

```sh
cargo install espflash
```

Edit the Wi-Fi credentials in `.cargo/config.toml` (`SSID`, `PASSWORD`, under
`[env]`) before building.

## Build / check / flash

```sh
cargo check            # fast type-check, no codegen
cargo build --release
espflash flash --release --monitor   # add --port /dev/ttyUSB0 if it doesn't auto-detect
```

The board logs "MPU6050 was found!", runs the 15s calibration pause, connects
to Wi-Fi, prints its IP, then starts the HTTP server. Visit that IP in a
browser for the same live gesture display as the original; `/read_gesture`
is the polled plain-text endpoint.
