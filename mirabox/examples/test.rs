use std::{error::Error, time::Duration};

use image::{DynamicImage, ImageBuffer, Rgb};
use mirabox::{
    DeviceEvent, EventParser, MiraboxN4Pro, PHYSICAL_BUTTON_IMAGE_FORMAT,
    VIRTUAL_BUTTON_IMAGE_FORMAT,
};

const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(1);
const DIGIT_GLYPHS: [[&str; 5]; 10] = [
    ["111", "1 1", "1 1", "1 1", "111"],
    [" 1 ", "11 ", " 1 ", " 1 ", "111"],
    ["111", "  1", "111", "1  ", "111"],
    ["111", "  1", "111", "  1", "111"],
    ["1 1", "1 1", "111", "  1", "  1"],
    ["111", "1  ", "111", "  1", "111"],
    ["111", "1  ", "111", "1 1", "111"],
    ["111", "  1", "  1", "  1", "  1"],
    ["111", "1 1", "111", "1 1", "111"],
    ["111", "1 1", "111", "  1", "111"],
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let Some(device) = MiraboxN4Pro::connect().await? else {
        eprintln!("No MiraBox N4 Pro found. Connect it, then run this example again.");
        return Ok(());
    };

    let firmware = device.firmware_version().unwrap_or("unknown").trim_end_matches('\0');
    println!(
        "Communication succeeded: serial {}, firmware {firmware}",
        device.serial_number()
    );

    for virtual_button in 0..4 {
        device
            .set_image(
                virtual_button,
                VIRTUAL_BUTTON_IMAGE_FORMAT,
                test_image(virtual_button, VIRTUAL_BUTTON_IMAGE_FORMAT.size),
            )
            .await?;
    }
    for button in 0..10 {
        let key = MiraboxN4Pro::physical_button_image_key(button)
            .expect("the test only addresses physical buttons 0 through 9");
        device
            .set_image(
                key,
                PHYSICAL_BUTTON_IMAGE_FORMAT,
                test_image(button, PHYSICAL_BUTTON_IMAGE_FORMAT.size),
            )
            .await?;
    }
    device.flush().await?;

    println!("Uploaded test images to 14 button displays (10 physical, 4 virtual).");
    println!("Listening for raw and enriched events; press Control-C to exit.");

    let mut event_parser = EventParser::new();
    let mut keep_alive = tokio::time::interval(KEEP_ALIVE_INTERVAL);
    keep_alive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = keep_alive.tick() => device.keep_alive().await?,
            raw_input = device.read_raw_input() => {
                if let Some(raw_input) = raw_input? {
                    match DeviceEvent::try_from(raw_input) {
                        Ok(event) => {
                            println!("event: {event:?}");
                            for event in event_parser.parse(event) {
                                println!("\x1b[1menriched: {event:?}\x1b[0m");
                            }
                        }
                        Err(raw_input) => println!("\x1b[31mraw: {raw_input:?}\x1b[0m"),
                    }
                }
            }
        }
    }
}

fn test_image(key: u8, (width, height): (usize, usize)) -> DynamicImage {
    let (width, height) = (width as u32, height as u32);
    let color = [
        key.wrapping_mul(47).wrapping_add(40),
        key.wrapping_mul(83).wrapping_add(90),
        key.wrapping_mul(131).wrapping_add(140),
    ];
    let border = 6;

    let mut image = ImageBuffer::from_fn(width, height, |x, y| {
        if x < border || y < border || x >= width - border || y >= height - border {
            Rgb([255, 255, 255])
        } else {
            Rgb(color)
        }
    });
    draw_button_number(&mut image, key, width, height);
    DynamicImage::ImageRgb8(image)
}

fn draw_button_number(
    image: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    number: u8,
    width: u32,
    height: u32,
) {
    let scale = (width.min(height) / 10).max(1);
    let glyph_width = 3 * scale;
    let glyph_height = 5 * scale;
    let origin_x = (width - glyph_width) / 2;
    let origin_y = (height - glyph_height) / 2;

    for (row, pattern) in DIGIT_GLYPHS[number as usize].iter().enumerate() {
        for (column, pixel) in pattern.bytes().enumerate() {
            if pixel == b'1' {
                for y in 0..scale {
                    for x in 0..scale {
                        image.put_pixel(
                            origin_x + column as u32 * scale + x,
                            origin_y + row as u32 * scale + y,
                            Rgb([255, 255, 255]),
                        );
                    }
                }
            }
        }
    }
}
