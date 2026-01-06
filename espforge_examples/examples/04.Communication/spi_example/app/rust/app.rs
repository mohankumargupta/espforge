#[warn(unused_variables)]

use crate::Context;

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    logger.info("SPI Example");
    let received = ctx.components.my_spi.write_read(3);
    logger.info("Sent for value of register 0x03");
    logger.info("received:");
    logger.info(received);
}

pub fn forever(ctx: &mut Context) {

}
