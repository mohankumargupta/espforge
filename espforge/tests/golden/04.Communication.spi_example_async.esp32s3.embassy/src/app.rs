#![allow(unused_variables)]

use crate::Context;
use embassy_executor::Spawner;

#[embassy_executor::task]
async fn transfer_task(ctx: &'static Context) {
    let spi = component!(ctx, spi0);
    let logger = ctx.logger;

    let mut buffer = [0x03u8, 0x00];
    let sent_byte = buffer[0];
    match spi.transfer_in_place(&mut buffer).await {
        Ok(_) => {
            logger.info(format_args!("Sent {}", sent_byte));
            logger.info(format_args!("Received: {}", buffer[1]))
        },
        Err(_) => logger.info("spi transfer failed"),
    }
}

pub async fn setup(ctx: &'static Context, spawner: Spawner) {
    spawner.spawn(transfer_task(ctx).expect("transfer task failed"));
}

pub async fn forever(ctx: &'static Context) {
    ctx.delay.delay_ms(1000).await;
}
