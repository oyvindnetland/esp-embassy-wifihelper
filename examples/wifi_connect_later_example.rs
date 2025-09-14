#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_net::dns::DnsQueryType;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_embassy_wifihelper::WifiStack;
use esp_hal::{clock::CpuClock, timer::timg::TimerGroup};
use esp_wifi::wifi::ClientConfiguration;
use esp_println as _; // global logger
#[cfg(feature = "log")]
use log::info;
#[cfg(feature = "defmt")]
use defmt::info;

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

static CHANNEL: Channel<CriticalSectionRawMutex, ClientConfiguration, 1> = Channel::new();

#[embassy_executor::task]
async fn delayed_connect_msg() {
    Timer::after(Duration::from_millis(5000)).await;
    let client_config = ClientConfiguration {
        ssid: SSID.try_into().unwrap(),
        password: PASSWORD.try_into().unwrap(),
        ..Default::default()
    };
    info!("Sending connect msg");
    let _ = CHANNEL.send(client_config).await;
}

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    #[cfg(feature = "log")]
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_bootloader_esp_idf::esp_app_desc!();
    esp_alloc::heap_allocator!(size: 72 * 1024);

    let timg1 = TimerGroup::new(peripherals.TIMG1);
    esp_hal_embassy::init(timg1.timer0);

    let wifi = WifiStack::new_connect_later(
        spawner,
        peripherals.WIFI,
        peripherals.TIMG0,
        peripherals.RNG,
        CHANNEL.receiver(),
    );

    spawner.spawn(delayed_connect_msg()).ok();

    info!("Wifi initialized, waiting for connection...");
    let config = wifi.wait_for_connected().await.unwrap();
    info!("Wifi connected with IP: {}", config.address);

    let res = wifi
        .dns_query("www.google.com", DnsQueryType::A)
        .await
        .unwrap();
    info!("dns lookup of www.google.com: {:?}", res);

    loop {
        info!("tick");
        Timer::after(Duration::from_millis(1000)).await;
    }
}
