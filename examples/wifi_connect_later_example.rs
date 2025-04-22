#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_futures::select::select;
use embassy_net::dns::DnsQueryType;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_embassy_wifihelper::WifiStack;
use esp_hal::{clock::CpuClock, timer::timg::TimerGroup};
use esp_println::println;
use esp_wifi::wifi::ClientConfiguration;
use log::info;

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

static CHANNEL: Channel<CriticalSectionRawMutex, ClientConfiguration, 1> = Channel::new();

async fn wait_connect(wifi: &WifiStack) {
    let config = wifi.wait_for_connected().await;
    info!("Wifi connected with IP: {}", config.unwrap().address);
    let res = wifi.dns_query("www.google.com", DnsQueryType::A).await;
    println!("dns: {:?}", res);
}

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
    esp_println::logger::init_logger_from_env();
    let mut config = esp_hal::Config::default();
    config.cpu_clock = CpuClock::max();
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(72 * 1024);

    let timg1 = TimerGroup::new(peripherals.TIMG1);
    esp_hal_embassy::init(timg1.timer0);

    let wifi = WifiStack::new_connect_later(
        spawner,
        peripherals.WIFI,
        peripherals.TIMG0,
        peripherals.RNG,
        peripherals.RADIO_CLK,
        CHANNEL.receiver(),
    );

    loop {
        match select(wait_connect(&wifi), delayed_connect_msg()).await {
            embassy_futures::select::Either::First(_) => break,
            embassy_futures::select::Either::Second(_) => {}
        }
    }

    info!("connected, starts sleeping");
    loop {
        Timer::after(Duration::from_millis(1000)).await;
    }
}
