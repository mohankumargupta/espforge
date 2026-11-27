use crate::{service, Context};
use embassy_executor::Spawner;

/// Runs once at startup: wait for Wi-Fi, then say what we are about to do.
pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    ctx.logger.info("Waiting for Wi-Fi connection...");
    service!(ctx, home).wait_connected().await;
    ctx.logger.info("Connected. Ready for WebSocket communication.");
}

/// Per-tick loop: open a session, say hello, read the echo, close, sleep.
pub async fn forever(ctx: &'static Context) {
    let ws = service!(ctx, ws);
    // The session holds the socket's own buffers for as long as it lives.
    let mut conn_buf = [0u8; 4096];
    let mut rx_buf = [0u8; 512];

    // Not `echo.websocket.org`: it answers port 80 with a 301 to TLS and the
    // upgrade checks the reply's status. Not `echo.websocket.events`: that host
    // no longer resolves, so the client reports a DNS error.
    match ws.connect("ws://websocket-echo.com/", &mut conn_buf).await {
        Ok(mut session) => {
            ctx.logger.info("Upgraded to WebSocket. Sending greeting...");
            if let Err(e) = session.send_text("Hello from ESP32!").await {
                ctx.logger.warn(format_args!("send failed: {e:?}"));
            }

            match session.recv(&mut rx_buf).await {
                Ok(msg) => {
                    if let Ok(text) = core::str::from_utf8(msg.payload) {
                        ctx.logger.info(format_args!("Received echo: {text}"));
                    }
                }
                Err(e) => ctx.logger.warn(format_args!("recv failed: {e:?}")),
            }

            let _ = session.close(1000).await;
        }
        Err(err) => ctx
            .logger
            .error(format_args!("WebSocket connection failed: {err:?}")),
    }

    ctx.delay.delay_ms(10000).await;
}
