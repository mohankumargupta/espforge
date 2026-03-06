use crate::{ Context, component };
use embassy_executor::Spawner;

pub async fn setup(ctx: &mut Context, _spawner: Spawner) {
    ctx.logger.info("WiFi HTTP Example");

    let http = component!(http);

    match http.get("http://example.com").await {
        Ok(response) => {
            ctx.logger
                .info(format_args!("GET status: {}", response.status));
            if response.is_ok() {
                if let Some(text) = response.text() {
                    ctx.logger.info(format_args!("{}", text));
                } else {
                    ctx.logger.info("(response body was truncated to 2048 bytes)");
                }
            }
        }
        Err(e) => {
            ctx.logger.info(format_args!("GET failed: {}", e));
        }
    }
}

pub async fn forever(ctx: &mut Context) {
    ctx.delay.delay_ms(60_000).await;
}
