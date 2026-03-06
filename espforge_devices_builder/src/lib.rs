pub mod ft6206;
pub mod ili9341;
pub mod ssd1306;

pub fn init() {
    // Force the MSVC linker to retain these modules by referencing them
    let _ = std::hint::black_box(&ft6206::FT6206Plugin);
    let _ = std::hint::black_box(&ili9341::ILI9341Plugin);
    let _ = std::hint::black_box(&ssd1306::SSD1306Plugin);
}