#![allow(unused_variables)]

use crate::Context;
use embassy_executor::Spawner;

// Two tasks share one `I2cBus<Async>` (design §20.4). The async mutex in the
// bus serializes their I2C transactions safely across tasks. I2C_TICK signals
// the monitor task to print a heartbeat after each full register scan.
crate::signal!(I2C_TICK);

/// Reads all 6 registers from the I2C chip at address 0x42, verifying each
/// against the expected regmap values {0, 10, 20, 30, 40, 50} set up in
/// chip.zig. Uses `write_read` for the idiomatic I2C register-read pattern:
/// START + write(addr, [reg_index]) + REPEATED-START + read(addr, buf).
#[embassy_executor::task]
async fn reader_task(ctx: &'static Context) {
    let i2c = component!(ctx, i2c0);
    let logger = ctx.logger;
    let expected: [u8; 6] = [0, 10, 20, 30, 40, 50];
    loop {
        for reg in 0..6u8 {
            let mut buf = [0u8; 1];
            match i2c.write_read(0x42, &[reg], &mut buf).await {
                Ok(_) => {
                    if buf[0] == expected[reg as usize] {
                        logger.info(format_args!("reg[{}]={} ✓", reg, buf[0]));
                    } else {
                        logger.info(format_args!(
                            "reg[{}]={} (expected {})",
                            reg, buf[0], expected[reg as usize]
                        ));
                    }
                }
                Err(_) => logger.info(format_args!("reg[{}] read failed", reg)),
            }
            ctx.delay.delay_ms(100).await;
        }
        I2C_TICK.signal(());
        ctx.delay.delay_ms(2000).await;
    }
}

/// Logs a heartbeat after each full register scan, proving two tasks can
/// coexist on one shared I2C bus.
#[embassy_executor::task]
async fn monitor_task(ctx: &'static Context) {
    loop {
        I2C_TICK.wait().await;
        ctx.logger.info("register scan cycle complete");
    }
}

pub async fn setup(ctx: &'static Context, spawner: Spawner) {
    spawner.spawn(reader_task(ctx).expect("reader_task spawn failed"));
    spawner.spawn(monitor_task(ctx).expect("monitor_task spawn failed"));
}

pub async fn forever(ctx: &'static Context) {
    // Work happens in the spawned tasks; the main loop just yields.
    ctx.delay.delay_ms(1000).await;
}
