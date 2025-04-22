#![cfg_attr(not(test), no_std)]

use embassy_executor::Spawner;
use embassy_net::{
    dns::{self, DnsQueryType},
    tcp::{ConnectError, TcpSocket},
    IpAddress, Runner, Stack, StackResources, StaticConfigV4,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Receiver};
use embassy_time::{Duration, Timer};
use esp_hal::{
    peripheral::Peripheral,
    peripherals::{RADIO_CLK, RNG, TIMG0, WIFI},
    rng::Rng,
    timer::timg::TimerGroup,
};

use esp_wifi::{
    init,
    wifi::{
        ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiStaDevice,
        WifiState,
    },
    EspWifiController,
};
#[cfg(feature = "esp32c3")]
use esp_wifi_sys::include::esp_wifi_set_max_tx_power;
use heapless::{String, Vec};
use log::{info, warn};

macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

pub struct WifiStack {
    pub stack: Stack<'static>,
}

impl WifiStack {
    fn new_internal(
        spawner: Spawner,
        wifi: impl Peripheral<P = WIFI> + 'static,
        timg0: impl Peripheral<P = TIMG0> + esp_hal::timer::timg::TimerGroupInstance,
        rng: impl Peripheral<P = RNG>,
        radio_clk: RADIO_CLK,
        ssid: Option<String<32>>,
        password: Option<String<64>>,
        rx: Option<Receiver<'static, CriticalSectionRawMutex, ClientConfiguration, 1>>,
    ) -> Self {
        let timg0 = TimerGroup::new(timg0);
        let mut rng = Rng::new(rng);
        let init = &*mk_static!(
            EspWifiController<'static>,
            init(timg0.timer0, rng.clone(), radio_clk).unwrap()
        );

        let (wifi_interface, controller) =
            esp_wifi::wifi::new_with_mode(&init, wifi, WifiStaDevice).unwrap();
        let config = embassy_net::Config::dhcpv4(Default::default());
        let seed = (rng.random() as u64) << 32 | rng.random() as u64;

        let (stack, runner) = &mut *mk_static!(
            (
                Stack<'static>,
                Runner<'static, WifiDevice<'_, WifiStaDevice>>
            ),
            embassy_net::new(
                wifi_interface,
                config,
                mk_static!(StackResources<3>, StackResources::<3>::new()),
                seed
            )
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
        Self { stack: *stack }
    }

    pub fn new(
        spawner: Spawner,
        wifi: impl Peripheral<P = WIFI> + 'static,
        timg0: impl Peripheral<P = TIMG0> + esp_hal::timer::timg::TimerGroupInstance,
        rng: impl Peripheral<P = RNG>,
        radio_clk: RADIO_CLK,
        ssid: String<32>,
        password: String<64>,
    ) -> Self {
        Self::new_internal(
            spawner,
            wifi,
            timg0,
            rng,
            radio_clk,
            Some(ssid),
            Some(password),
            None,
        )
    }

    pub fn new_connect_later(
        spawner: Spawner,
        wifi: impl Peripheral<P = WIFI> + 'static,
        timg0: impl Peripheral<P = TIMG0> + esp_hal::timer::timg::TimerGroupInstance,
        rng: impl Peripheral<P = RNG>,
        radio_clk: RADIO_CLK,
        rx: Receiver<'static, CriticalSectionRawMutex, ClientConfiguration, 1>,
    ) -> Self {
        Self::new_internal(spawner, wifi, timg0, rng, radio_clk, None, None, Some(rx))
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
    ) -> Result<Vec<IpAddress, 1>, dns::Error> {
        let res = self.stack.dns_query(addr, query_type).await?;
        Ok(res)
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
    client_configuration: ClientConfiguration,
    retries: usize,
) {
    let client_config = Configuration::Client(client_configuration);

    for _ in 0..retries {
        match esp_wifi::wifi::wifi_state() {
            WifiState::StaConnected => {
                // wait until we're no longer connected
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                Timer::after(Duration::from_millis(5000)).await
            }
            _ => {}
        }
        if !matches!(controller.is_started(), Ok(true)) {
            controller.set_configuration(&client_config).unwrap();
            info!("Starting wifi");
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
            Ok(_) => info!("Wifi connected!"),
            Err(e) => {
                warn!("Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }

    warn!(
        "Failed to connect to {} after {} retries",
        client_config.as_client_conf_ref().unwrap().ssid,
        retries
    );
}

#[embassy_executor::task]
async fn connection(controller: WifiController<'static>, ssid: String<32>, password: String<64>) {
    let client_config = ClientConfiguration {
        ssid,
        password,
        ..Default::default()
    };

    connecting_loop(controller, client_config, 10).await;
}

#[embassy_executor::task]
async fn connection_later(
    controller: WifiController<'static>,
    rx: Receiver<'static, CriticalSectionRawMutex, ClientConfiguration, 1>,
) {
    let client_config = rx.receive().await;
    connecting_loop(controller, client_config, 10).await;
}

#[embassy_executor::task]
async fn net_task(runner: &'static mut Runner<'static, WifiDevice<'static, WifiStaDevice>>) {
    runner.run().await
}
