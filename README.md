# Gesture Recognition Glove

A wearable ESP32 glove that reads hand gestures and displays the translated
sign live on a web page served straight from the board.

5 flex sensors (one per finger) feed voltage dividers into the ESP32's ADC,
and an MPU6050 accelerometer/gyroscope adds motion data for gestures that
need hand movement rather than just a finger pose (e.g. "Hello!"). Live
sensor readings are matched against a set of gesture thresholds, and the
current match is served over Wi-Fi as a small page that polls the board
twice a second.

## Hardware

- ESP32 DOIT DevKit V1
- 5x flex sensor (4.5"), each its own voltage divider, on ADC1 (GPIO34/35/33/32/39)
- MPU6050 accelerometer + gyroscope over I2C (GPIO21/22)
- 18650 Li-ion battery + shield
- Custom PCB and 3D-printed enclosure — see [`PCB/`](PCB) and [`Enclosure/`](Enclosure)

## Firmware

- **[`Algorithm-rust/`](Algorithm-rust)** — current firmware: a `no_std` Rust
  rewrite on `esp-hal`, no ESP-IDF or C toolchain needed. See its README for
  build/flash instructions.
- The original Arduino/PlatformIO (C++) firmware, PCB Gerbers, and the full
  build write-up are preserved in the
  [v0.1 release](https://github.com/FrazzledTurtle/Gesture-Recognition-Glove/releases/tag/v0.1)
  and no longer kept in the current tree.
