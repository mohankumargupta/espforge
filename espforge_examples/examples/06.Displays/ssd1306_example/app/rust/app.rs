use crate::Context;

pub fn setup(ctx: &mut Context) {
    ctx.logger.info("Initializing OLED...");
    
    // Access the device defined in example.yaml under 'devices: oled'
    let oled = &mut ctx.devices.oled;

    oled.init();
    oled.clear();
    oled.print(0, 0, "Hello Espforge!");
    oled.flush();
    
    ctx.logger.info("OLED Initialized");
}

pub fn forever(ctx: &mut Context) {
    ctx.delay.delay_ms(1000);
}
