#![no_std]
#![no_main]

use defmt::info;
#[cfg(any(feature = "esp32c6", feature = "esp32s3"))]
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_net::dns::DnsQueryType;
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_embassy_wifihelper::WifiStack;
use esp_hal as _;
#[cfg(any(feature = "esp32c3", feature = "esp32c6"))]
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::{clock::CpuClock, timer::timg::TimerGroup};
#[cfg(any(
    feature = "esp32",
    feature = "esp32c2",
    feature = "esp32c3",
    feature = "esp32s2"
))]
use esp_println as _;

esp_bootloader_esp_idf::esp_app_desc!();

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

#[esp_rtos::main]
async fn main(spawner: Spawner) {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    #[cfg(any(feature = "esp32c3", feature = "esp32c6"))]
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(
        timg0.timer0,
        #[cfg(any(feature = "esp32c3", feature = "esp32c6"))]
        sw_int.software_interrupt0,
    );

    esp_alloc::heap_allocator!(size: 72 * 1024);

    let wifi = WifiStack::new(
        spawner,
        peripherals.WIFI,
        SSID.try_into().unwrap(),
        PASSWORD.try_into().unwrap(),
    );

    info!("Wifi initialized, waiting for connection...");
    let config = wifi.wait_for_connected().await.unwrap();
    info!("Wifi connected with IP: {}", config.address);

    let res = wifi
        .dns_query("www.google.com", DnsQueryType::A)
        .await
        .unwrap();
    info!("dns lookup of www.google.com: {}", res);

    loop {
        info!("tick");
        Timer::after(Duration::from_millis(1000)).await;
    }
}
