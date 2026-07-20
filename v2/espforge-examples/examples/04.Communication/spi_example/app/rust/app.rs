#![allow(unused_variables)]

use crate::embedded_hal::spi::Operation;

use crate::Context;

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    let spi = component!(ctx, spi_tester);

    logger.info("SPI Example");

    // A device owns its own chip-select pin (design §20.5): the backend wraps
    // the shared `SpiBus` in a `SpiDevice` with the CS from the component spec.
    // Full-duplex SPI reads and writes simultaneously, so we use
    // `TransferInPlace` to send 0x03 and read the response in one transaction.
    let mut buffer = [0x03u8, 0x00];
    match spi.transaction(&mut [Operation::TransferInPlace(&mut buffer)]) {
        Ok(_) => {
            logger.info("Sent 0x03");
            logger.info(format_args!("Received: {}", buffer[1]));
        }
        Err(_) => {
            logger.info("SPI Transfer failed");
        }
    }
}

pub fn forever(ctx: &mut Context) {
    let delay = ctx.delay;
    delay.delay_ms(1000);
}
