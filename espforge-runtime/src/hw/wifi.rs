//! Wi-Fi station support: esp-radio + embassy-net behind one small API.
//!
//! Compiled only with the `wifi` feature (which implies `embassy`), so a
//! project without a `<wifi>` radio never pays for the stack.
//!
//! This file is the only place that touches the esp-radio API (still beta), so
//! a version bump means editing here, not the project templates.
//!
//! Prerequisites, all provided by the generated `main`: the `esp-rtos`
//! scheduler is running, an `esp-alloc` heap exists, and the `WIFI` peripheral
//! is moved in by value.

use core::cell::RefCell;

use embassy_executor::Spawner;
use embassy_net::{Runner, Stack, StackResources};
use embassy_time::{Duration, Timer};
use esp_hal::peripherals::WIFI;
use esp_radio::wifi::{
    AuthenticationMethodConfig, Config, ControllerConfig, Interface, WifiController,
    sta::StationConfig,
};
use static_cell::StaticCell;

/// Station settings, written into `generated.rs` from `<wifi_station>`.
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

/// Handle to a running station connection. `Copy`, so it can sit on a
/// device and be reached through the generated `Context`.
#[derive(Clone, Copy)]
pub struct Wifi {
    stack: Stack<'static>,
}

/// The `static` one `<wifi_station>` needs: its own embassy-net stack-resource
/// pool.
///
/// The generated `generated.rs` declares one per station (`__HOME_STORAGE`), so
/// two stations never fight over one pool inside `StaticCell::init`.
pub struct WifiStorage {
    resources: StaticCell<StackResources<4>>,
}

impl WifiStorage {
    pub const fn new() -> Self {
        Self {
            resources: StaticCell::new(),
        }
    }
}

impl Default for WifiStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl Wifi {
    /// Create the controller, configure the station, build the network stack
    /// (DHCP) and spawn the connection and network tasks. Returns right away;
    /// use [`Wifi::wait_connected`] to wait for an address.
    ///
    /// `storage` is the station's own `static`, so `start_in` can never
    /// initialise one pool twice.
    pub fn start_in(
        storage: &'static WifiStorage,
        spawner: &Spawner,
        wifi: WIFI<'static>,
        config: WifiConfig,
    ) -> Result<Self, WifiError> {
        let mut controller = WifiController::new(wifi, ControllerConfig::default())?;
        let interface = Interface::station();

        let authentication = if config.password.is_empty() {
            AuthenticationMethodConfig::Open
        } else {
            AuthenticationMethodConfig::Wpa2Personal(config.password.try_into()?)
        };
        let station = StationConfig::default()
            .with_ssid(config.ssid.try_into()?)
            .with_authentication(authentication);
        controller.set_config(&Config::Station(station))?;

        let rng = esp_hal::rng::Rng::new();
        let seed = (u64::from(rng.random()) << 32) | u64::from(rng.random());

        let (stack, runner) = embassy_net::new(
            interface,
            embassy_net::Config::dhcpv4(Default::default()),
            storage.resources.init(StackResources::new()),
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

/// A `<wifi_station>` service: the station's credentials, and once started,
/// the running connection handle.
///
/// It constructs in `Components::new` — no peripheral, no `unsafe` — and
/// *starts* from `main` after `esp_rtos::start`, because the radio needs the
/// scheduler and the heap. `start` therefore takes `&self` and parks the handle
/// behind a `RefCell`, and `Components` never needs to be `mut`.
pub struct WifiStation {
    config: WifiConfig,
    storage: &'static WifiStorage,
    handle: RefCell<Option<Wifi>>,
}

impl WifiStation {
    /// `storage` is the station's own generated `static`; the station keeps the
    /// reference so `start` needs no extra argument.
    pub fn new(config: WifiConfig, storage: &'static WifiStorage) -> Self {
        Self {
            config,
            storage,
            handle: RefCell::new(None),
        }
    }

    /// Bring the radio up and spawn the association and network tasks. Returns
    /// right away; use [`WifiStation::wait_connected`] to wait for an address.
    pub fn start(&self, wifi: WIFI<'static>, spawner: &Spawner) -> Result<(), WifiError> {
        let handle = Wifi::start_in(self.storage, spawner, wifi, self.config)?;
        *self.handle.borrow_mut() = Some(handle);
        Ok(())
    }

    /// The running handle. Panics if the station has not been started — the
    /// generated `main` always starts it before building the services.
    fn handle(&self) -> Wifi {
        match &*self.handle.borrow() {
            Some(handle) => *handle,
            None => panic!("wifi station not started"),
        }
    }

    /// The embassy-net stack, for network services (HTTP, HTTPS).
    pub fn stack(&self) -> Stack<'static> {
        self.handle().stack()
    }

    /// Wait until the link is up and DHCP has assigned an address.
    pub async fn wait_connected(&self) {
        self.handle().wait_connected().await;
    }
}

/// The interface a network provider satisfies.
///
/// A service (HTTP, WebSocket, NTP) rides on a provider's embassy-net stack and
/// never names a concrete provider type. `<wifi_station>` is the first
/// implementor; an Ethernet or cellular provider implements the same trait.
pub trait NetworkInterface {
    /// The running stack. The generated code starts a provider before it builds
    /// its clients, so this never observes a not-yet-started handle.
    fn stack(&self) -> Stack<'static>;
}

impl NetworkInterface for Wifi {
    fn stack(&self) -> Stack<'static> {
        Wifi::stack(self)
    }
}

impl NetworkInterface for WifiStation {
    fn stack(&self) -> Stack<'static> {
        WifiStation::stack(self)
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
async fn net_task(mut runner: Runner<'static, Interface>) {
    runner.run().await
}
