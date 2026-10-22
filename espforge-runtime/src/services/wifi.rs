//! Wi-Fi station support: esp-radio + embassy-net behind one small API.
//!
//! This file is the only place that touches the esp-radio API (still beta),
//! so a version bump means editing here, not the project templates.
//!
//! Prerequisites, all provided by the generated `main`: the `esp-rtos`
//! scheduler is running and an `esp-alloc` heap exists.

use embassy_executor::Spawner;
use embassy_net::{Runner, Stack, StackResources};
use embassy_time::{Duration, Timer};
use esp_hal::peripherals::WIFI;
use esp_radio::wifi::{
    AuthenticationMethod, Config, ControllerConfig, Interface, WifiController, sta::StationConfig,
};
use static_cell::StaticCell;

/// Station settings, written into `generated.rs` from `<wifi>` in `# services`.
#[derive(Clone, Copy)]
pub struct WifiConfig {
    pub ssid: &'static str,
    /// Empty means an open network.
    pub password: &'static str,
}

#[derive(Debug)]
pub enum WifiError {
    Radio(esp_radio::wifi::WifiError),
    /// An embassy task could not be spawned (pool exhausted).
    Spawn,
}

impl From<esp_radio::wifi::WifiError> for WifiError {
    fn from(e: esp_radio::wifi::WifiError) -> Self {
        WifiError::Radio(e)
    }
}

/// Handle to a running station connection. `Copy`, so it can sit on `Context`.
#[derive(Clone, Copy)]
pub struct Wifi {
    stack: Stack<'static>,
}

static RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();

impl Wifi {
    /// Create the controller, configure the station, build the network stack
    /// (DHCP) and spawn the connection and network tasks. Returns right away;
    /// use [`Wifi::wait_connected`] to wait for an address.
    pub fn start(
        spawner: &Spawner,
        wifi: WIFI<'static>,
        config: WifiConfig,
    ) -> Result<Self, WifiError> {
        let (mut controller, interfaces) = esp_radio::wifi::new(wifi, ControllerConfig::default())?;

        let mut station = StationConfig::default()
            .with_ssid(config.ssid.into())
            .with_password(config.password.into());
        if config.password.is_empty() {
            station = station.with_auth_method(AuthenticationMethod::None);
        }
        controller.set_config(&Config::Station(station))?;

        let rng = esp_hal::rng::Rng::new();
        let seed = (u64::from(rng.random()) << 32) | u64::from(rng.random());

        let (stack, runner) = embassy_net::new(
            interfaces.station,
            embassy_net::Config::dhcpv4(Default::default()),
            RESOURCES.init(StackResources::new()),
            seed,
        );

        spawner.spawn(connection_task(controller).map_err(|_| WifiError::Spawn)?);
        spawner.spawn(net_task(runner).map_err(|_| WifiError::Spawn)?);

        Ok(Self { stack })
    }

    /// The embassy-net stack, for sockets (TCP, UDP, DNS...).
    pub fn stack(&self) -> Stack<'static> {
        self.stack
    }

    /// Wait until the link is up and DHCP has assigned an address.
    pub async fn wait_connected(&self) {
        self.stack.wait_link_up().await;
        self.stack.wait_config_up().await;
    }
}

/// Keeps the station associated: connect, wait for a drop, retry.
#[embassy_executor::task]
async fn connection_task(mut controller: WifiController<'static>) {
    loop {
        match controller.connect_async().await {
            Ok(_) => {
                log::info!("wifi: connected");
                let _ = controller.wait_for_disconnect_async().await;
                log::warn!("wifi: disconnected, retrying");
            }
            Err(e) => {
                log::warn!("wifi: connect failed: {:?}", e);
                Timer::after(Duration::from_secs(5)).await;
            }
        }
    }
}

/// Drives the network stack.
#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface<'static>>) {
    runner.run().await
}
