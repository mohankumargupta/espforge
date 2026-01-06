#[warn(unused_variables)]

use crate::Context;

pub fn setup(ctx: &mut Context) {
  ctx.logger.info("UART Example");
  let uart = ctx.components.my_uart; 
  uart.write("Hello\n");
}

pub fn forever(ctx: &mut Context) {
    let logger = ctx.logger;
    let uart = ctx.components.my_uart;
    if my_uart.buffer_until_newline() {        
        logger.info("Message received:");
        logger.info(my_uart.get_buffered_string());
        uart.clear_buffer()
    }
    delay.delay_millis(10);
}
