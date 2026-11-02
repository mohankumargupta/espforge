#![allow(unused_variables)]

use crate::{Context, embedded_hal::spi::Operation};
use embassy_executor::Spawner;

#[embassy_executor::task]
async fn transfer_task(ctx: &'static Context) {
    let spi = component!(ctx, spi0);
    let logger = ctx.logger;

    //ctx.delay.delay_ms(50).await;

    let mut rx = [0x00u8];
    let sent_byte = 0x03u8;

    match spi
        .transaction(&mut [
            Operation::DelayNs(10_000),     // CS setup delay
            Operation::Write(&[sent_byte]), // Send register index 3
            //Operation::DelayNs(5_000),    // Inter-byte processing delay
            Operation::Read(&mut rx), // Clock out the register value (30)
        ])
        .await
    {
        Ok(_) => {
            logger.info(format_args!("Sent {}", sent_byte));
            logger.info(format_args!("Received: {}", rx[0])); // Prints 30
        }
        Err(_) => logger.info("spi transfer failed"),
    }
}

pub async fn setup(ctx: &'static Context, spawner: Spawner) {
    spawner.spawn(transfer_task(ctx).expect("transfer task failed"));
}

pub async fn forever(ctx: &'static Context) {
    ctx.delay.delay_ms(1000).await;
}
