#![allow(unused_variables)]

use crate::Context;

pub async fn setup(ctx: &'static Context, _spawner: embassy_executor::Spawner) {
    let logger = ctx.logger;
    logger.info("async WS2812 RGB strip example");

    // The channel is built after boot: `Devices::new` is synchronous, so the
    // async constructor runs here. The guard may be held across `.await`.
    let mut slot = device!(ctx, strip);
    let strip = match slot.init().await {
        Ok(strip) => strip,
        Err(error) => {
            logger.error(format_args!("WS2812 init failed: {error}"));
            return;
        }
    };

    let pixels = [[32, 0, 0], [0, 32, 0], [0, 0, 32]];
    if strip.write(&pixels).await.is_err() {
        logger.error("WS2812 frame failed");
    }
}

pub async fn forever(ctx: &'static Context) {
    ctx.delay.delay_ms(500).await;
}
