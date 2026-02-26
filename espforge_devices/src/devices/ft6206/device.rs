use core::mem;
use embedded_hal::i2c::I2c;
//extern crate alloc;
//use alloc::vec::Vec; // espforge projects enable alloc

pub const FT6206_ADDR: u8 = 0x38;
pub const REG_MODE: u8 = 0x00;
pub const REG_NUM_TOUCHES: u8 = 0x02;
pub const REG_TOUCH1_XH: u8 = 0x03;
pub const REG_TOUCH2_XH: u8 = 0x09;

const MAX_RAW: u16 = 4095;

#[derive(Debug, Clone, Copy)]
pub struct TouchPoint {
    pub x: u16,
    pub y: u16,
    pub raw_x: u16,
    pub raw_y: u16,
    pub id: u8,
    pub event: TouchEvent,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TouchEvent {
    Press,
    Release,
    Move,
    Unknown,
}

pub struct FT6206<I> {
    i2c: I,
    address: u8,
    swap_xy: bool,
    mirror_x: bool,
    mirror_y: bool,
    screen_width: u16,  // screen width for scaling
    screen_height: u16, // screen height for scaling
    x_min: u16,
    x_max: u16,
    y_min: u16,
    y_max: u16,
}

impl<I: I2c> FT6206<I> {
    pub fn new(
        i2c: I,
        address: u8,
        swap_xy: bool,
        mirror_x: bool,
        mirror_y: bool,
        screen_width: u16,
        screen_height: u16,
        x_min: u16,
        x_max: u16,
        y_min: u16,
        y_max: u16,
    ) -> Self {
        Self {
            i2c,
            address,
            swap_xy,
            mirror_x,
            mirror_y,
            screen_width,
            screen_height,
            x_min,
            x_max,
            y_min,
            y_max,
        }
    }

    pub fn init(&mut self) -> Result<(), I::Error> {
        // Configure normal operating mode (0x00)
        self.write_register(REG_MODE, 0x00)?;
        Ok(())
    }

    fn write_register(&mut self, reg: u8, value: u8) -> Result<(), I::Error> {
        self.i2c.write(self.address, &[reg, value])
    }

    fn read_register(&mut self, reg: u8, buffer: &mut [u8]) -> Result<(), I::Error> {
        self.i2c.write_read(self.address, &[reg], buffer)
    }

    pub fn read_touches(&mut self) -> Result<[Option<TouchPoint>; 2], I::Error> {
        let mut buf = [0u8; 1];
        self.read_register(REG_NUM_TOUCHES, &mut buf)?;
        let num = (buf[0] & 0x0F) as usize;
        let num = num.min(2);

        let mut points = [None; 2];

        for i in 0..num {
            let base_reg = if i == 0 { REG_TOUCH1_XH } else { REG_TOUCH2_XH };
            let mut data = [0u8; 6];
            self.read_register(base_reg, &mut data)?;

            let raw_x = ((data[0] & 0x0F) as u16) << 8 | data[1] as u16;
            let raw_y = ((data[2] & 0x0F) as u16) << 8 | data[3] as u16;

            let mut x = raw_x;
            let mut y = raw_y;

            if self.mirror_x {
                x = self.map_range(raw_x, self.x_min, self.x_max, self.screen_width, 0);
            } else {
                x = self.map_range(raw_x, self.x_min, self.x_max, 0, self.screen_width);
            }

            if self.mirror_y {
                y = self.map_range(raw_y, self.y_min, self.y_max, self.screen_height, 0);
            } else {
                y = self.map_range(raw_y, self.y_min, self.y_max, 0, self.screen_height);
            }

            if self.swap_xy {
                core::mem::swap(&mut x, &mut y);
            }

            let id = i as u8;
            let event = match (data[0] >> 6) & 0x03 {
                0 => TouchEvent::Press,
                1 => TouchEvent::Release,
                2 => TouchEvent::Move,
                _ => TouchEvent::Unknown,
            };

            points[i] = Some(TouchPoint {
                x,
                y,
                raw_x,
                raw_y,
                id,
                event,
            });
        }

        Ok(points)
    }

    pub fn detect_press(&mut self) -> Option<TouchPoint> {
        self.read_touches()
            .ok()?
            .iter()
            .flatten()
            .find(|p| matches!(p.event, TouchEvent::Press))
            .copied()
    }

    pub fn map_range(
        &self,
        value: u16,
        in_min: u16,
        in_max: u16,
        out_min: u16,
        out_max: u16,
    ) -> u16 {
        let value = value.clamp(in_min, in_max);
        if in_max == in_min {
            return out_min;
        }
        // Use i64 to handle mirrored ranges where out_max < out_min
        let value = value as i64;
        let in_min = in_min as i64;
        let in_max = in_max as i64;
        let out_min = out_min as i64;
        let out_max = out_max as i64;

        let mapped = (value - in_min) * (out_max - out_min) / (in_max - in_min) + out_min;
        mapped.clamp(0, u16::MAX as i64) as u16
    }
}
