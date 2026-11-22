use embassy_executor::Spawner;
use esp_hal::gpio::Level;
use espforge_runtime::PulseCode;

use crate::Context;

/// A short NEC-style lead-in, then a stop bit. The channel emits the pulse
/// codes; the app decides what they mean.
const FRAME: [PulseCode; 2] = [
    PulseCode::new(Level::High, 560, Level::Low, 1690),
    PulseCode::new(Level::High, 560, Level::Low, 560),
];

pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    let mut ir = device!(ctx, ir_tx);
    if ir.transmit(&FRAME).await.is_err() {
        ctx.logger.info("RMT transmit failed");
    }
}

pub async fn forever(ctx: &'static Context) {
    ctx.delay.delay_ms(1000).await;
}
