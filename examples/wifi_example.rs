#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_net::dns::DnsQueryType;
//use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_embassy_wifihelper::WifiStack;
use esp_hal::{clock::CpuClock, timer::timg::TimerGroup};
use esp_println::println;
use log::info;

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    esp_println::logger::init_logger_from_env();
    let mut config = esp_hal::Config::default();
    config.cpu_clock = CpuClock::max();
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(72 * 1024);

    let timg1 = TimerGroup::new(peripherals.TIMG1);
    esp_hal_embassy::init(timg1.timer0);

    let wifi = WifiStack::new(
        spawner,
        peripherals.WIFI,
        peripherals.TIMG0,
        peripherals.RNG,
        peripherals.RADIO_CLK,
        SSID.try_into().unwrap(),
        PASSWORD.try_into().unwrap(),
    );

    let config = wifi.wait_for_connected().await.unwrap();
    info!("Wifi connected with IP: {}", config.address);

    let res = wifi.dns_query("www.google.com", DnsQueryType::A).await;
    println!("dns: {:?}", res);

    loop {
        println!("tick");
        Timer::after(Duration::from_millis(1000)).await;
    }
}
