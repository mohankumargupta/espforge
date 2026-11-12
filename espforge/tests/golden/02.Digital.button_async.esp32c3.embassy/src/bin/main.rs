#![no_std]
#![no_main]
use embassy_executor::Spawner;
use esp_backtrace as _;
use esp_hal::timer::timg::TimerGroup;
use button_async::{app, Context, Devices, PeripheralRegistry, CTX};
use button_async::Services;
#[esp_rtos::main]
async fn main(spawner: Spawner) {
    esp_println::logger::init_logger_from_env();
    esp_println::print!("\x1b[20h");
    esp_bootloader_esp_idf::esp_app_desc!();
    let peripherals = esp_hal::init(esp_hal::Config::default());
    #[allow(unused_mut)]
    let mut registry = PeripheralRegistry::new(peripherals);
    let logger = espforge_runtime::Logger::new();
    let delay = espforge_runtime::Delay::new();
    let devices = match Devices::new(&mut registry) {
        Ok(devices) => devices,
        Err(error) => {
            logger.error(format_args!("{}", error));
            loop {
                esp_hal::delay::Delay::new().delay_millis(1_000);
            }
        }
    };
    let timg0 = TimerGroup::new(registry.peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, registry.peripherals.FROM_CPU_INTR0);
    let services = Services {};
    let ctx = Context {
        logger,
        delay,
        devices,
        services,
    };
    let ctx = CTX.init(ctx);
    app::setup(ctx, spawner).await;
    loop {
        app::forever(ctx).await;
    }
}
