#![no_std]
#![no_main]
use embassy_executor::Spawner;
use esp_backtrace as _;
use esp_hal::timer::timg::TimerGroup;
use ota_update::{app, Context, Devices, PeripheralRegistry, CTX};
use ota_update::Services;
use ota_update::generated::__OTA_STORAGE;
#[esp_rtos::main]
async fn main(spawner: Spawner) {
    esp_println::logger::init_logger_from_env();
    esp_println::print!("\x1b[20h");
    esp_bootloader_esp_idf::esp_app_desc!();
    let peripherals = esp_hal::init(esp_hal::Config::default());
    esp_alloc::heap_allocator!(size : 72 * 1024);
    #[allow(unused_mut)]
    let mut registry = PeripheralRegistry::new(peripherals);
    let logger = espforge_runtime::Logger::new();
    let delay = espforge_runtime::Delay::new();
    let devices = Devices {};
    let timg0 = TimerGroup::new(registry.peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, registry.peripherals.FROM_CPU_INTR0);
    let services = Services::new();
    services.home.start(registry.peripherals.WIFI, &spawner).expect("wifi start failed");
    let ota = match espforge_runtime::Ota::new_in(
        &__OTA_STORAGE,
        registry.peripherals.FLASH,
        true,
    ) {
        Ok(ota) => ota,
        Err(error) => {
            logger.error(format_args!("{}", error));
            loop {
                esp_hal::delay::Delay::new().delay_millis(1_000);
            }
        }
    };
    let ctx = Context {
        logger,
        delay,
        devices,
        services,
        ota,
    };
    let ctx = CTX.init(ctx);
    app::setup(ctx, spawner).await;
    loop {
        app::forever(ctx).await;
    }
}
