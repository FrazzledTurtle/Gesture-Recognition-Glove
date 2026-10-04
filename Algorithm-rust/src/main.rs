#![no_std]
#![no_main]

use embassy_futures::join::join4;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Runner, StackResources};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};
use embedded_io_async::Write as _;
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::analog::adc::{Adc, AdcChannel, AdcConfig, AdcPin, Attenuation};
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::rng::Rng;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::Blocking;
use esp_println::println;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{AuthenticationMethodConfig, Config as WifiModeConfig, ControllerConfig, Interface, WifiController};

const MPU6050_ADDR: u8 = 0x68;
const REG_PWR_MGMT_1: u8 = 0x6B;
const REG_ACCEL_XOUT_H: u8 = 0x3B;

// MPU6050 LSB/g at the chip's default +-2g full-scale accelerometer range.
const ACCEL_SENSITIVITY: f32 = 16384.0;

/// Minimal MPU6050 register access. The published `mpu6050` crate still targets
/// embedded-hal 0.2's blocking traits, which `esp-hal`'s I2C driver (embedded-hal
/// 1.0) doesn't implement, so this talks to the chip directly instead.
struct Mpu6050<'d> {
    i2c: I2c<'d, Blocking>,
}

impl<'d> Mpu6050<'d> {
    fn new(i2c: I2c<'d, Blocking>) -> Self {
        Self { i2c }
    }

    async fn init(&mut self) -> Result<(), esp_hal::i2c::master::Error> {
        // The chip starts in sleep mode; clear PWR_MGMT_1's sleep bit to wake it.
        self.i2c.write(MPU6050_ADDR, &[REG_PWR_MGMT_1, 0x00])?;
        Timer::after(Duration::from_millis(100)).await;
        Ok(())
    }

    fn read_accel_g(&mut self) -> Result<(f32, f32, f32), esp_hal::i2c::master::Error> {
        let mut buf = [0u8; 6];
        self.i2c.write_read(MPU6050_ADDR, &[REG_ACCEL_XOUT_H], &mut buf)?;
        Ok((
            i16::from_be_bytes([buf[0], buf[1]]) as f32 / ACCEL_SENSITIVITY,
            i16::from_be_bytes([buf[2], buf[3]]) as f32 / ACCEL_SENSITIVITY,
            i16::from_be_bytes([buf[4], buf[5]]) as f32 / ACCEL_SENSITIVITY,
        ))
    }
}

esp_bootloader_esp_idf::esp_app_desc!();

// Network credentials, set in `.cargo/config.toml`.
const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

// Flex sensor voltage divider.
const VCC: f32 = 3.3;
const ADC_MAX: f32 = 4095.0;
const R_DIVIDER: f32 = 10_000.0;

// g -> m/s^2, to match Adafruit_MPU6050's SI-unit sensor events that the
// original firmware's accelerometer thresholds were tuned against.
const STANDARD_GRAVITY: f32 = 9.80665;

static INDEX_HTML: &str = include_str!("index.html");

/// The currently displayed gesture, shared between the sensor loop and the
/// HTTP server. Always one of the `&'static str` literals `classify_gesture`
/// can return.
static GESTURE: Mutex<CriticalSectionRawMutex, &'static str> = Mutex::new("IDLE");

macro_rules! mk_static {
    ($t:ty, $val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        STATIC_CELL.uninit().write($val)
    }};
}

#[esp_hal::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    let peripherals = esp_hal::init(esp_hal::Config::default());

    esp_alloc::heap_allocator!(size: 64 * 1024);

    let mut led = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());

    // MPU6050 on the default I2C0 pins (SDA = GPIO21, SCL = GPIO22).
    let i2c = I2c::new(peripherals.I2C0, I2cConfig::default().with_frequency(Rate::from_khz(100)))
        .unwrap()
        .with_sda(peripherals.GPIO21)
        .with_scl(peripherals.GPIO22);
    let mut mpu = Mpu6050::new(i2c);
    match mpu.init().await {
        Ok(()) => println!("MPU6050 was found!"),
        Err(e) => println!("Failed to find the MPU6050 chip: {e:?}"),
    }

    // The 5 flex sensors, all on ADC1 channels (ADC2 can't be used together
    // with Wi-Fi on the ESP32), matching the original GPIO assignment.
    let mut adc_config = AdcConfig::new();
    let flex1 = adc_config.enable_pin(peripherals.GPIO34, Attenuation::_11dB);
    let flex2 = adc_config.enable_pin(peripherals.GPIO35, Attenuation::_11dB);
    let flex3 = adc_config.enable_pin(peripherals.GPIO33, Attenuation::_11dB);
    let flex4 = adc_config.enable_pin(peripherals.GPIO32, Attenuation::_11dB);
    let flex5 = adc_config.enable_pin(peripherals.GPIO39, Attenuation::_11dB);
    let adc1 = Adc::new(peripherals.ADC1, adc_config);

    calibration_delay().await;

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    let station_config = WifiModeConfig::Station(
        StationConfig::default()
            .with_ssid(SSID.try_into().unwrap())
            .with_authentication(AuthenticationMethodConfig::Wpa2Personal(PASSWORD.try_into().unwrap())),
    );

    println!("Starting wifi");
    let wifi_interface = Interface::station();
    let controller = WifiController::new(
        peripherals.WIFI,
        ControllerConfig::default().with_initial_config(station_config),
    )
    .unwrap();
    println!("Wifi configured and started!");

    let net_config = embassy_net::Config::dhcpv4(Default::default());
    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    let (stack, runner) = embassy_net::new(
        wifi_interface,
        net_config,
        mk_static!(StackResources<3>, StackResources::<3>::new()),
        seed,
    );

    led.set_high();

    join4(
        connection(controller),
        net_task(runner),
        sensor_loop(mpu, adc1, flex1, flex2, flex3, flex4, flex5),
        http_loop(stack),
    )
    .await;

    unreachable!()
}

async fn connection(mut controller: WifiController<'static>) -> ! {
    println!("start connection task");
    loop {
        println!("About to connect...");
        match controller.connect_async().await {
            Ok(info) => {
                println!("Wifi connected to {info:?}");
                let info = controller.wait_for_disconnect_async().await.ok();
                println!("Disconnected: {info:?}");
            }
            Err(e) => println!("Failed to connect to wifi: {e:?}"),
        }
        Timer::after(Duration::from_millis(5000)).await;
    }
}

async fn net_task(mut runner: Runner<'static, Interface>) -> ! {
    runner.run().await
}

async fn calibration_delay() {
    println!("Starting calibration.");
    Timer::after(Duration::from_millis(15_000)).await;
    println!("Calibration was successful.");
}

async fn sensor_loop<P1, P2, P3, P4, P5>(
    mut mpu: Mpu6050<'static>,
    mut adc1: Adc<'static, esp_hal::peripherals::ADC1<'static>, Blocking>,
    mut flex1: AdcPin<P1, esp_hal::peripherals::ADC1<'static>>,
    mut flex2: AdcPin<P2, esp_hal::peripherals::ADC1<'static>>,
    mut flex3: AdcPin<P3, esp_hal::peripherals::ADC1<'static>>,
    mut flex4: AdcPin<P4, esp_hal::peripherals::ADC1<'static>>,
    mut flex5: AdcPin<P5, esp_hal::peripherals::ADC1<'static>>,
) -> !
where
    P1: AdcChannel,
    P2: AdcChannel,
    P3: AdcChannel,
    P4: AdcChannel,
    P5: AdcChannel,
{
    let mut last_printed: &'static str = "";

    loop {
        let acc_x = read_accel_x_ms2(&mut mpu);

        let r1 = flex_resistance(&mut adc1, &mut flex1);
        let r2 = flex_resistance(&mut adc1, &mut flex2);
        let r3 = flex_resistance(&mut adc1, &mut flex3);
        let r4 = flex_resistance(&mut adc1, &mut flex4);
        let r5 = flex_resistance(&mut adc1, &mut flex5);

        let previous = *GESTURE.lock().await;
        let new_gesture = classify_gesture([r1, r2, r3, r4, r5], acc_x, previous);

        if new_gesture != last_printed {
            println!("{new_gesture}");
            last_printed = new_gesture;
        }
        *GESTURE.lock().await = new_gesture;

        Timer::after(Duration::from_millis(500)).await;
    }
}

async fn http_loop(stack: embassy_net::Stack<'static>) -> ! {
    let mut rx_buffer = [0u8; 2048];
    let mut tx_buffer = [0u8; 2048];

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(10)));

        if let Err(e) = socket.accept(80).await {
            println!("accept error: {e:?}");
            continue;
        }

        let mut request = [0u8; 512];
        let n = match socket.read(&mut request).await {
            Ok(n) if n > 0 => n,
            _ => {
                socket.close();
                continue;
            }
        };

        let result = if request[..n].starts_with(b"GET /read_gesture") {
            let gesture = *GESTURE.lock().await;
            respond(&mut socket, PLAIN_TEXT_HEADER, gesture.as_bytes()).await
        } else {
            respond(&mut socket, HTML_HEADER, INDEX_HTML.as_bytes()).await
        };
        if let Err(e) = result {
            println!("response error: {e:?}");
        }

        socket.close();
        let _ = socket.flush().await;
    }
}

const HTML_HEADER: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n";
const PLAIN_TEXT_HEADER: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n";

async fn respond(socket: &mut TcpSocket<'_>, header: &str, body: &[u8]) -> Result<(), embassy_net::tcp::Error> {
    socket.write_all(header.as_bytes()).await?;
    socket.write_all(body).await?;
    Ok(())
}

fn read_accel_x_ms2(mpu: &mut Mpu6050<'static>) -> f32 {
    match mpu.read_accel_g() {
        Ok((x, _y, _z)) => x * STANDARD_GRAVITY,
        Err(e) => {
            println!("MPU6050 accel read failed: {e:?}");
            0.0
        }
    }
}

fn flex_resistance<P: AdcChannel>(
    adc1: &mut Adc<'static, esp_hal::peripherals::ADC1<'static>, Blocking>,
    pin: &mut AdcPin<P, esp_hal::peripherals::ADC1<'static>>,
) -> f32 {
    let raw = nb::block!(adc1.read_oneshot(pin)).unwrap_or(0) as f32;
    let voltage = raw * VCC / ADC_MAX;
    R_DIVIDER * (VCC / voltage - 1.0)
}

fn in_range(value: f32, low: f32, high: f32) -> bool {
    value >= low && value <= high
}

// Poor man's datasets (same thresholds as the original firmware).
fn classify_gesture(flex_r: [f32; 5], acc_x: f32, previous: &'static str) -> &'static str {
    let [r1, r2, r3, r4, r5] = flex_r;
    let mut gesture = previous;

    if in_range(r1, 3000.0, 8500.0)
        && in_range(r2, 3000.0, 8500.0)
        && in_range(r3, 3000.0, 8500.0)
        && in_range(r4, 3000.0, 8500.0)
        && in_range(r5, 3000.0, 8500.0)
    {
        gesture = "IDLE";
    }
    if in_range(r1, 7000.0, 18000.0)
        && in_range(r2, 3000.0, 7000.0)
        && in_range(r3, 7000.0, 18000.0)
        && in_range(r4, 7000.0, 18000.0)
        && in_range(r5, 7000.0, 18000.0)
    {
        gesture = "1";
    }
    if in_range(r1, 7000.0, 18000.0)
        && in_range(r2, 3000.0, 7000.0)
        && in_range(r3, 3000.0, 7000.0)
        && in_range(r4, 7000.0, 18000.0)
        && in_range(r5, 7000.0, 18000.0)
    {
        gesture = "2";
    }
    if in_range(r1, 3000.0, 7000.0)
        && in_range(r2, 3000.0, 7000.0)
        && in_range(r3, 3000.0, 7000.0)
        && in_range(r4, 7000.0, 18000.0)
        && in_range(r5, 7000.0, 18000.0)
    {
        gesture = "3";
    }
    if in_range(r1, 3000.0, 7000.0)
        && in_range(r2, 3000.0, 7000.0)
        && in_range(r3, 7000.0, 18000.0)
        && in_range(r4, 7000.0, 18000.0)
        && in_range(r5, 3000.0, 8500.0)
    {
        gesture = "I LOVE YOU!";
    }
    if in_range(r1, 3000.0, 7000.0)
        && in_range(r2, 7000.0, 18000.0)
        && in_range(r3, 7000.0, 18000.0)
        && in_range(r4, 7000.0, 18000.0)
        && in_range(r5, 3000.0, 8500.0)
    {
        gesture = "Name";
    }
    if in_range(acc_x, -10.50, -3.50) {
        gesture = "My!";
    }
    if in_range(acc_x, 5.50, 10.50) {
        gesture = "HELLO!";
    }

    gesture
}
