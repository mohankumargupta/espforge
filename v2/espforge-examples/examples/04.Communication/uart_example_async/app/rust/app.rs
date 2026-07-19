#[allow(unused_variables)]

use crate::Context;
use embassy_executor::Spawner;

// UART is point-to-point, but two tasks can still contend on the same
// `&'static UartDevice<Async>` (design §20.5). The async mutex serializes them:
// a sender task and a receiver task share one peripheral safely.
crate::signal!(UART_RX);

#[embassy_executor::task]
async fn echo_task(ctx: &'static Context) {
    let uart = component!(ctx, uart0);
    let mut buf = [0u8; 64];
    loop {
        match uart.read_line(&mut buf).await {
            Ok(n) if n > 0 => {
                let line = core::str::from_utf8(&buf[..n]).unwrap_or("");
                uart.write_str(line).await.ok();
                UART_RX.signal(());
            }
            Ok(_) => {}
            Err(_) => ctx.logger.info("uart read error"),
        }
    }
}

#[embassy_executor::task]
async fn sender_task(ctx: &'static Context) {
    let uart = component!(ctx, uart0);
    let logger = ctx.logger;
    loop {
        UART_RX.wait().await;
        logger.info("uart line echoed");
        ctx.delay.delay_ms(500).await;
    }
}

pub async fn setup(ctx: &'static Context, spawner: Spawner) {
    spawner.spawn(echo_task(ctx).unwrap());
    spawner.spawn(sender_task(ctx).unwrap());
    component!(ctx, uart0)
        .write_str("Async UART ready\n")
        .await
        .ok();
}

pub async fn forever(ctx: &'static Context) {
    ctx.delay.delay_ms(1000).await;
}
