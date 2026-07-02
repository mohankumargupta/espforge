use crate::{component, Context};
use embassy_executor::Spawner;

pub async fn setup(ctx: &mut Context, _spawner: Spawner) {
    let logger = ctx.logger;
    logger.info("Starting HTTPS example");
    let https = component!(https);

    match https.get("https://httpbin.org/ip").await {
        Ok(response) => {
            ctx.logger.info(format_args!("status: {}", response.status));

            if let Some(text) = response.text() {
                ctx.logger.info(text);
            }
        }
        Err(e) => {
            ctx.logger.info(format_args!("HTTPS error: {:?}", e));
            panic!("https request failed");
        }
    }
}

pub async fn forever(ctx: &mut Context) {

}



