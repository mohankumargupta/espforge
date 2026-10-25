#![allow(unused_variables)]

use crate::Context;
use embassy_executor::Spawner;

#[embassy_executor::task]
async fn transfer_task(ctx: &'static Context) {
    let i2c = component!(ctx, i2c0);
    let logger = ctx.logger;
    let mut buffer = [0x00u8];
    let sent_byte = 0x03u8;
    match i2c.write_read(0x42, &[sent_byte], &mut buffer).await {
        Ok(_) => {
            logger.info(format_args!("Sent {}", sent_byte));
            logger.info(format_args!("Received: {}", buffer[0]));
        },
        Err(_) => logger.info("i2c transfer failed"),
     }
}

pub async fn setup(ctx: &'static Context, spawner: Spawner) {
    spawner.spawn(transfer_task(ctx).expect("transfer_task spawn failed"));
}

pub async fn forever(ctx: &'static Context) {
    // Work happens in the spawned tasks; the main loop just yields.
    ctx.delay.delay_ms(1000).await;
}
