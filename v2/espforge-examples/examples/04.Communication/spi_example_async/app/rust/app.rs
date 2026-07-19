#![allow(unused_variables)]

use embedded_hal::spi::Operation;

use crate::{component, Context};
use embassy_executor::Spawner;

// Two tasks each drive their own `SpiDevice` on the *same* shared
// `SpiBus<Async>`. The bus mutex serializes the underlying `Spi` transfers;
// each device asserts only its own CS (design §20.5). This proves safe
// cross-task sharing of one SPI controller.
crate::signal!(SPI_TICK);

#[embassy_executor::task]
async fn transfer_task(ctx: &'static Context) {
    let spi = component!(ctx, spi0);
    let logger = ctx.logger;
    loop {
        let mut buffer = [0x03u8, 0x00];
        match spi.transaction(&mut [Operation::TransferInPlace(&mut buffer)]).await {
            Ok(_) => logger.info(format_args!("spi rx: {}", buffer[1])),
            Err(_) => logger.info("spi transfer failed"),
        }
        SPI_TICK.signal(());
        ctx.delay.delay_ms(1000).await;
    }
}

#[embassy_executor::task]
async fn monitor_task(ctx: &'static Context) {
    loop {
        SPI_TICK.wait().await;
        ctx.logger.info("spi cycle done");
    }
}

pub async fn setup(ctx: &'static Context, spawner: Spawner) {
    spawner.spawn(transfer_task(ctx)).unwrap();
    spawner.spawn(monitor_task(ctx)).unwrap();
}

pub async fn forever(_ctx: &'static Context) {
    embassy_futures::yield_now().await;
}
