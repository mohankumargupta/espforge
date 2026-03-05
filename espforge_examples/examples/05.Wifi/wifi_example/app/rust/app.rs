use crate::Context;
use embassy_executor::Spawner;

pub async fn setup(ctx: &mut Context, _spawner: Spawner) {
    ctx.logger.info("WiFi HTTP Example");

    // GET request
    match ctx.wifi.get("http://example.com").await {
        Ok(response) => {
            ctx.logger
                .info(format_args!("GET status: {}", response.status));
            if response.is_ok() {
                if let Some(text) = response.text() {
                    ctx.logger.info(format_args!("Body: {}", text));
                }
                if response.truncated {
                    ctx.logger.info("(response body was truncated to 2048 bytes)");
                }
            }
        }
        Err(e) => {
            ctx.logger.info(format_args!("GET failed: {}", e));
        }
    }

    // POST request
    // match ctx
    //     .wifi
    //     .post("http://httpbin.org/post", b"{\"hello\":\"world\"}")
    //     .await
    // {
    //     Ok(response) => {
    //         ctx.logger
    //             .info(format_args!("POST status: {}", response.status));
    //     }
    //     Err(e) => {
    //         ctx.logger.info(format_args!("POST failed: {}", e));
    //     }
    // }
}

pub async fn forever(ctx: &mut Context) {
    ctx.delay.delay_ms(60_000).await;
}
