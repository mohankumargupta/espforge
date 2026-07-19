
use crate::{Context};
use embassy_executor::Spawner;

// Cross-task signal async inter-task communication. Producer signals; consumer waits.
crate::signal!(BUTTON_PRESSED);

/// Spawned embassy task: waits for the button (edge-triggered) and
/// signals the LED task to toggle. Runs for the lifetime of the firmware.
#[embassy_executor::task]
async fn button_task(ctx: &'static Context) {
    loop {
        ctx.components.button.wait_for_pressed().await;
        BUTTON_PRESSED.signal(());
    }
}

/// Runs once at startup. Spawn the button-watching task.
pub async fn setup(ctx: &'static Context, spawner: Spawner) {
    spawner.spawn(button_task(ctx).unwrap());
}

/// Per-tick loop. Waits for a press signal, toggles the LED, then debounces.
pub async fn forever(ctx: &'static Context) {
    BUTTON_PRESSED.wait().await;
    ctx.components.red_led.toggle();
    // Debounce: ignore further presses for a short window.
    ctx.delay.delay_ms(100).await;
}
