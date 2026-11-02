use crate::{component, Context};
use embassy_executor::Spawner;

/// Runs once at startup: wait for the association and DHCP, then report the
/// address.
pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    let logger = ctx.logger;
    let station = component!(ctx, home);

    logger.info("Wi-Fi example: connecting...");
    station.wait_connected().await;

    match station.stack().config_v4() {
        Some(config) => logger.info(format_args!("Connected, IP address: {}", config.address)),
        None => logger.info("Connected, but no IPv4 address"),
    }
}

/// The connection and network tasks run in the background.
pub async fn forever(ctx: &'static Context) {
    ctx.delay.delay_ms(1000).await;
}
