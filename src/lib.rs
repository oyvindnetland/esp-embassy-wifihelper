#![cfg_attr(not(test), no_std)]

use embassy_executor::Spawner;
use embassy_net::{Runner, Stack, StackResources, StaticConfigV4};
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
use heapless::String;
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
    pub fn new(
        spawner: Spawner,
        wifi: impl Peripheral<P = WIFI> + 'static,
        timg0: impl Peripheral<P = TIMG0>,
        rng: impl Peripheral<P = RNG>,
        radio_clk: RADIO_CLK,
        ssid: String<32>,
        password: String<64>,
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

        spawner.spawn(connection(controller, ssid, password)).ok();
        spawner.spawn(net_task(runner)).ok();
        Self { stack: *stack }
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
}

#[embassy_executor::task]
async fn connection(
    mut controller: WifiController<'static>,
    ssid: String<32>,
    password: String<64>,
) {
    let client_config = Configuration::Client(ClientConfiguration {
        ssid,
        password,
        ..Default::default()
    });

    loop {
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

        match controller.connect_async().await {
            Ok(_) => info!("Wifi connected!"),
            Err(e) => {
                warn!("Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

#[embassy_executor::task]
async fn net_task(runner: &'static mut Runner<'static, WifiDevice<'static, WifiStaDevice>>) {
    runner.run().await
}
