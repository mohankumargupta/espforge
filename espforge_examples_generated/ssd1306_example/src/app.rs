#[allow(unused_variables)]

use crate::{Context, device};

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    let oled = device!(oled);

    logger.info("Initializing OLED...");
    
    oled.init();
    oled.clear();
    oled.print(0, 0, "Hello Espforge!");
    oled.flush();
    
    logger.info("OLED Initialized");
}

pub fn forever(ctx: &mut Context) {
    let delay = ctx.delay;
    
    delay.delay_ms(1000);
}
