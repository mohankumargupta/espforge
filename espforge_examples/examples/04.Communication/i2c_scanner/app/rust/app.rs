#[warn(unused_variables)]

use crate::Context;

pub fn setup(ctx: &mut Context) {
  ctx.logger.info("i2c scanner Example");
      while address <= 127 {
        if my_i2c.probe(address) {
            log.info("Found I2C device at address:");
            log.print_hex(address);
            found = true;
        }
        address = address + 1;
        delay.delay_millis(10)
    }
    if !found {
        log.info("No i2c devices found");
    }
}

pub fn forever(ctx: &mut Context) {
  delay.delay_millis(1000);
}
