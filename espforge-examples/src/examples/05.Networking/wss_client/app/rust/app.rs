use crate::{component, service, Context};
use embassy_executor::Spawner;

/// Runs once at startup: wait for Wi-Fi, then say what we are about to do.
pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    ctx.logger.info("Waiting for Wi-Fi connection...");
    component!(ctx, home).wait_connected().await;
    ctx.logger.info("Connected. Streaming status over secure WebSocket.");
}

/// Per-tick loop: open a TLS session, send one JSON frame, sleep.
pub async fn forever(ctx: &'static Context) {
    let wss = service!(ctx, secure_ws);
    // Socket buffers first, then the two TLS record buffers, all borrowed for
    // the life of the session.
    let mut conn_buf = [0u8; 8192];
    let mut rx_buf = [0u8; 512];
    let payload = r#"{"sensor":"temp","value":24.5}"#;

    match wss
        .connect("wss://echo.websocket.events/", &mut conn_buf)
        .await
    {
        Ok(mut session) => {
            ctx.logger.info("TLS WebSocket open. Sending telemetry...");
            if let Err(e) = session.send_text(payload).await {
                ctx.logger.warn(format_args!("send failed: {e:?}"));
            }

            match session.recv(&mut rx_buf).await {
                Ok(msg) => ctx.logger.info(format_args!("Ack: {} bytes", msg.payload.len())),
                Err(e) => ctx.logger.warn(format_args!("stream error: {e:?}")),
            }

            let _ = session.close(1000).await;
        }
        Err(err) => ctx
            .logger
            .error(format_args!("WSS connection failed: {err:?}")),
    }

    ctx.delay.delay_ms(5000).await;
}
