use crate::{component, Context};
use embassy_executor::Spawner;

// Two tasks share one `I2cBus<Async>` (design §20.4). The async mutex in the
// bus serializes their transactions safely across tasks; neither task can see
// the other's partial write. SCAN_DONE signals the logger task to print a tick.
crate::signal!(SCAN_DONE);

/// Scans the bus once, then signals. Proves two tasks can coexist on one bus.
#[embassy_executor::task]
async fn scan_task(ctx: &'static Context) {
    loop {
        let i2c = component!(ctx, i2c0);
        let logger = ctx.logger;
        for address in 1..127 {
            match i2c.write(address, &[]).await {
                Ok(_) => logger.info(format_args!("found 0x{:02x}", address)),
                Err(_) => {}
            }
            ctx.delay.delay_ms(10).await;
        }
        SCAN_DONE.signal(());
        ctx.delay.delay_ms(2000).await;
    }
}

/// Just logs a heartbeat between scans, also touching the (shared) logger bus.
#[embassy_executor::task]
async fn heartbeat_task(ctx: &'static Context) {
    loop {
        SCAN_DONE.wait().await;
        ctx.logger.info("scan cycle complete");
    }
}

pub async fn setup(ctx: &'static Context, spawner: Spawner) {
    spawner.spawn(scan_task(ctx)).unwrap();
    spawner.spawn(heartbeat_task(ctx)).unwrap();
}

pub async fn forever(_ctx: &'static Context) {
    // Work happens in the spawned tasks; the main loop just yields.
    embassy_futures::yield_now().await;
}
