use espforge_platform::logger;

pub fn setup(ctx: &mut Context) {
    let logger::init();
    logger.info("Hello World");
}

pub fn forever(ctx: &mut Context) {

}