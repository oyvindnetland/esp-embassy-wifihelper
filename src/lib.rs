#![cfg_attr(not(test), no_std)]

extern crate alloc;
use embassy_executor::Spawner;
use embassy_net::{
    tcp::{ConnectError, TcpSocket},
    IpAddress, Runner, Stack, StackResources, StaticConfigV4,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Receiver};
use embassy_time::{Duration, Timer};
use esp_hal::{peripherals::WIFI, rng::Rng};

use alloc::boxed::Box;
use alloc::string::String;
#[cfg(feature = "defmt")]
use defmt::warn;
use embassy_net::{dns, dns::DnsQueryType};
use esp_radio::{
    init,
    wifi::{self, ClientConfig, ModeConfig, WifiController, WifiDevice, WifiEvent},
    Controller,
};
#[cfg(feature = "esp32c3")]
use esp_wifi_sys::include::esp_wifi_set_max_tx_power;
#[cfg(feature = "log")]
use log::warn;

pub struct WifiStack {
    pub stack: Stack<'static>,
}

impl WifiStack {
    fn new_internal(
        spawner: Spawner,
        wifi: WIFI<'static>,
        ssid: Option<String>,
        password: Option<String>,
        rx: Option<Receiver<'static, CriticalSectionRawMutex, ModeConfig, 1>>,
    ) -> Self {
        let rng = Rng::new();

        let init: &'static Controller<'static> = Box::leak(Box::new(init().unwrap()));
        let (controller, interfaces) = wifi::new(&init, wifi, Default::default()).unwrap();

        let wifi_interface = interfaces.sta;

        let config = embassy_net::Config::dhcpv4(Default::default());
        let seed = (rng.random() as u64) << 32 | rng.random() as u64;

        let (stack, runner) = embassy_net::new(
            wifi_interface,
            config,
            Box::leak(Box::new(StackResources::<3>::new())),
            seed,
        );

        if ssid.is_some() && password.is_some() {
            spawner
                .spawn(connection(controller, ssid.unwrap(), password.unwrap()))
                .ok();
        } else if rx.is_some() {
            spawner
                .spawn(connection_later(controller, rx.unwrap()))
                .ok();
        } else {
            panic!("neither ssid/pass nor rx provided");
        }
        spawner.spawn(net_task(runner)).ok();
        Self { stack }
    }

    pub fn new(spawner: Spawner, wifi: WIFI<'static>, ssid: String, password: String) -> Self {
        Self::new_internal(spawner, wifi, Some(ssid), Some(password), None)
    }

    pub fn new_connect_later(
        spawner: Spawner,
        wifi: WIFI<'static>,
        rx: Receiver<'static, CriticalSectionRawMutex, ModeConfig, 1>,
    ) -> Self {
        Self::new_internal(spawner, wifi, None, None, Some(rx))
    }

    pub async fn wait_for_connected(&self) -> Option<StaticConfigV4> {
        while !self.stack.is_link_up() {
            Timer::after(Duration::from_millis(500)).await;
        }

        loop {
            if let Some(config) = self.stack.config_v4() {
                return Some(config);
            }
            Timer::after(Duration::from_millis(500)).await;
        }
    }

    pub async fn dns_query(
        &self,
        addr: &str,
        query_type: DnsQueryType,
    ) -> Result<IpAddress, dns::Error> {
        let res = self.stack.dns_query(addr, query_type).await?;
        Ok(res[0])
    }

    pub async fn make_and_connect_tcp_socket<'a>(
        &self,
        addr: IpAddress,
        port: u16,
        rx_buffer: &'a mut [u8],
        tx_buffer: &'a mut [u8],
    ) -> Result<TcpSocket<'a>, ConnectError> {
        let mut socket = TcpSocket::new(self.stack, rx_buffer, tx_buffer);
        socket.connect((addr, port)).await?;
        Ok(socket)
    }
}

async fn connecting_loop(
    mut controller: WifiController<'static>,
    client_configuration: ModeConfig,
    retries: usize,
) {
    for _ in 0..retries {
        if !matches!(controller.is_started(), Ok(true)) {
            controller.set_config(&client_configuration).unwrap();
            controller.start_async().await.unwrap();
        }
        #[cfg(feature = "esp32c3")]
        unsafe {
            // necessary to be able to establish a connection on esp32c3
            let res = esp_wifi_set_max_tx_power(36);
            if res != 0 {
                warn!("failed to set esp_wifi_set_max_tx_power {}", res);
            }
        }

        match controller.connect_async().await {
            Ok(_) => match esp_radio::wifi::sta_state() {
                wifi::WifiStaState::Connected => {
                    controller.wait_for_event(WifiEvent::StaDisconnected).await;
                    Timer::after(Duration::from_millis(5000)).await
                }
                _ => {}
            },
            Err(e) => {
                warn!("Failed to connect to wifi: {:?}", e);
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }

    warn!(
        "Failed to connect to {:?} after {} retries",
        client_configuration, retries
    );
}

#[embassy_executor::task]
async fn connection(controller: WifiController<'static>, ssid: String, password: String) {
    let client_config = ModeConfig::Client(
        ClientConfig::default()
            .with_ssid(ssid)
            .with_password(password),
    );

    connecting_loop(controller, client_config, 10).await;
}

#[embassy_executor::task]
async fn connection_later(
    controller: WifiController<'static>,
    rx: Receiver<'static, CriticalSectionRawMutex, ModeConfig, 1>,
) {
    let client_config = rx.receive().await;
    connecting_loop(controller, client_config, 10).await;
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}
