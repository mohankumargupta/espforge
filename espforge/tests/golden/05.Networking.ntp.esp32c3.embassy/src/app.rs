use crate::Context;
use embassy_executor::Spawner;

/// Runs once at startup: wait for Wi-Fi, then anchor the clock.
pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    ctx.logger.info("Waiting for Wi-Fi connection...");
    service!(ctx, home).wait_connected().await;

    // One query is enough: `sync` stores the server's Unix time, and `now`
    // advances it with the monotonic clock. `get_time()` still exists for a
    // one-shot query that should not touch the clock.
    ctx.logger.info("Connected. Synchronising the clock.");
    match service!(ctx, clock).sync().await {
        Ok(time) => ctx.logger.info(format_args!(
            "Clock set to {}.{:06} s since the Unix epoch (RTT {} us)",
            time.unix_seconds, time.microseconds, time.roundtrip_delay_us
        )),
        Err(error) => ctx.logger.warn(format_args!("NTP sync failed: {error:?}")),
    }
}

/// Per-tick loop: read the clock locally, no network involved.
pub async fn forever(ctx: &'static Context) {
    match service!(ctx, clock).now() {
        Ok(now) => {
            let seconds_in_day = now.seconds % 86400;
            ctx.logger.info(format_args!(
                "UTC {:02}:{:02}:{:02}.{:06} ({} s since the Unix epoch)",
                seconds_in_day / 3600,
                (seconds_in_day % 3600) / 60,
                seconds_in_day % 60,
                now.micros,
                now.seconds
            ));
        }
        Err(error) => ctx.logger.warn(format_args!("Clock not set: {error}")),
    }

    // A local read costs nothing; the one-minute figure only paces the log.
    ctx.delay.delay_ms(60000).await;
}
