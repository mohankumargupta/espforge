use crate::{Context};

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    let delay = ctx.delay;
    let i2c = component!(ctx, i2c0);

    logger.info("I2C Scanner Example");

    // Scan addresses 1 to 127. Try a 0-byte write to each address; if it ACKs
    // there is a device there. `I2cBus<Blocking>` exposes the idiomatic
    // `write` helper (design §20.5); the underlying bus is also reachable via
    // `bus()`.
    for address in 1..127 {
        match i2c.write(address, &[]) {
            Ok(_) => {
                logger.info(format_args!("Found device at address 0x{:02x}", address));
            }
            Err(_) => {}
        }
        delay.delay_ms(10);
    }

    logger.info("Scan complete");
}

pub fn forever(ctx: &mut Context) {
    ctx.delay.delay_ms(1000);
}
