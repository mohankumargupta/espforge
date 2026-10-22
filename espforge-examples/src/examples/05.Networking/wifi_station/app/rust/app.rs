use crate::Context;
use embassy_executor::Spawner;

pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    let logger = ctx.logger;
    let wifi = service!(ctx, wifi);

    logger.info("Wi-Fi example: connecting...");
    wifi.wait_connected().await;

    match wifi.stack().config_v4() {
        Some(config) => logger.info(format_args!("Connected, IP address: {}", config.address)),
        None => logger.info("Connected, but no IPv4 address"),
    }
}

pub async fn forever(ctx: &'static Context) {
    // The connection and network tasks run in the background.
    ctx.delay.delay_ms(1000).await;
}
