const wokwi = @import("wokwi-api.zig");

var chip: Chip = .{
    .uart = 0,
    .rx_idx = 0,
    .rx_buffer = .{0} ** 128,
};

fn printU8(num: u8) void {
    // Max u8 is 255 (3 digits) + 1 null terminator = 4 bytes
    var buffer: [4]u8 = undefined;
    var n = num;
    var len: usize = 0;

    // 1. Handle the 0 case explicitly
    if (n == 0) {
        buffer[0] = '0';
        buffer[1] = 0; // Null terminator
        wokwi.debugPrint(@ptrCast(&buffer));
        return;
    }

    // 2. Count the number of digits
    var temp = n;
    while (temp > 0) {
        temp /= 10;
        len += 1;
    }

    // 3. Fill buffer backwards (because % 10 gives the last digit)
    // Set the null terminator at the end of the digits
    buffer[len] = 0;

    var i: usize = len;
    while (n > 0) {
        i -= 1;
        buffer[i] = (n % 10) + '0';
        n /= 10;
    }

    // 4. Cast the fixed-size array pointer to a C-string pointer and print
    wokwi.debugPrint(@ptrCast(&buffer));
}

// --- Chip Logic ---

const Chip = extern struct {
    uart: wokwi.UARTDevId,
    rx_buffer: [128:0]u8,
    rx_idx: usize,
};

export fn on_rx_data(user_data: ?*anyopaque, byte: u8) callconv(.c) void {
    _ = user_data;
    //_ = byte;
    //printU8(byte);

    if (chip.rx_idx < chip.rx_buffer.len - 1) {
        chip.rx_buffer[chip.rx_idx] = byte;
        chip.rx_idx += 1;
    }
    //printU8(@intCast(chip.rx_idx));

    // newline from esp32 indicates they expect a reply
    if (byte == '\n') {
        // Null-terminate the string for debugPrint
        chip.rx_buffer[chip.rx_idx] = 0;
        //const rx_idx: u8 = @intCast(chip.rx_idx);
        //printU8(rx_idx);
        //wokwi.debugPrint(@ptrCast(&chip.rx_buffer));
        chip.rx_idx = 0;

        wokwi.debugPrint("Message received");
        wokwi.debugPrint(@ptrCast(&chip.rx_buffer));
        wokwi.debugPrint("End Message received");
        wokwi.debugPrint("Newline received. Sending Reply");
        const message = "world\n";
        _ = wokwi.uartWrite(chip.uart, message, message.len);
    }
}

export fn chipInit() callconv(.c) void {
    wokwi.debugPrint("UART Custom chip");
    const tx_pin = wokwi.pinInit("TX", wokwi.OUTPUT);
    const rx_pin = wokwi.pinInit("RX", wokwi.INPUT);

    const uart_config = wokwi.UARTConfig{
        .tx = tx_pin,
        .rx = rx_pin,
        .baud_rate = 9600,
        .rx_data = @constCast(&on_rx_data),
        .write_done = null,
        .user_data = @constCast(&chip),
    };

    chip.uart = wokwi.uartInit(uart_config);
}
