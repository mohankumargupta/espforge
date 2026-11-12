#![allow(unused_variables)]

use crate::Context;
use embassy_executor::Spawner;

/// Await the sensor's init, then read one sample.
///
/// The device is deferred because its bus is async: `Devices::new` builds the
/// endpoint and the factory, and this is where the driver's constructor runs.
async fn read_once(ctx: &'static Context) {
    let mut slot = device!(ctx, accel);
    let accel = match slot.init().await {
        Ok(accel) => accel,
        Err(error) => {
            ctx.logger.warn(format_args!("ADXL345 init failed: {error}"));
            return;
        }
    };

    match accel.read().await {
        Ok(reading) => {
            let (x, y, z) = reading.milli_g();
            ctx.logger.info(format_args!("ADXL345 {x} {y} {z} mg"));
        }
        Err(error) => ctx.logger.warn(format_args!("ADXL345 read failed: {error:?}")),
    }
}

pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    ctx.logger.info("SPI async example");
    read_once(ctx).await;
}

pub async fn forever(ctx: &'static Context) {
    ctx.delay.delay_ms(200).await;
    read_once(ctx).await;
}
