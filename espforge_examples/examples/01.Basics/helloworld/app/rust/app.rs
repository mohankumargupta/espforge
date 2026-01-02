use app::Context;

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;
    logger.info("Hello World");
}

pub fn forever(ctx: &mut Context) {

}