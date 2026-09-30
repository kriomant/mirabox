use std::{error::Error, time::Duration};

use image::{DynamicImage, ImageBuffer, Rgb};
use mirabox::{
    DeviceEvent, KnobDirection, MiraboxN4Pro, PHYSICAL_BUTTON_IMAGE_FORMAT,
    VIRTUAL_BUTTON_IMAGE_FORMAT,
};

const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(1);
const BRIGHTNESS_STEP: u8 = 5;
const MAX_LIGHT_BRIGHTNESS: u8 = 60;
const MAX_SCREEN_BRIGHTNESS: u8 = 100;
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

    let mut light_brightness = MAX_LIGHT_BRIGHTNESS / 2;
    let mut screen_brightness = MAX_SCREEN_BRIGHTNESS / 2;
    device.set_light_brightness(light_brightness).await?;
    device.set_screen_brightness(screen_brightness).await?;
    println!("Knob 1: backlight brightness (0–60), currently {light_brightness}");
    println!("Knob 2: screen brightness (0–100), currently {screen_brightness}");
    println!("Clockwise increases; counterclockwise decreases.");
    println!("Each detent changes brightness by {BRIGHTNESS_STEP}.");

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
    println!("Listening for events and raw reports; press Control-C to exit.");

    let mut keep_alive = tokio::time::interval(KEEP_ALIVE_INTERVAL);
    keep_alive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = keep_alive.tick() => device.keep_alive().await?,
            report_event = device.read_report_event() => {
                let (report, event) = report_event?;
                if let Some(event) = event {
                    print!("event: {event:?} ");
                }
                print!("\x1b[2m");
                for byte in visible_report(&report) {
                    print!(" {:02x?}", byte);
                }
                println!("\x1b[m");
                match event {
                    Some(DeviceEvent::KnobRotated { knob: 0, direction }) => {
                        let next = adjust_brightness(
                            light_brightness, direction, MAX_LIGHT_BRIGHTNESS
                        );
                        if next != light_brightness {
                            device.set_light_brightness(next).await?;
                            light_brightness = next;
                            println!("Backlight brightness: {light_brightness}/{MAX_LIGHT_BRIGHTNESS}");
                        }
                    }
                    Some(DeviceEvent::KnobRotated { knob: 1, direction }) => {
                        let next = adjust_brightness(
                            screen_brightness, direction, MAX_SCREEN_BRIGHTNESS
                        );
                        if next != screen_brightness {
                            device.set_screen_brightness(next).await?;
                            screen_brightness = next;
                            println!("Screen brightness: {screen_brightness}/{MAX_SCREEN_BRIGHTNESS}");
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn adjust_brightness(value: u8, direction: KnobDirection, max: u8) -> u8 {
    match direction {
        KnobDirection::Clockwise => value.saturating_add(BRIGHTNESS_STEP).min(max),
        KnobDirection::CounterClockwise => value.saturating_sub(BRIGHTNESS_STEP),
    }
}

fn visible_report(report: &[u8; 512]) -> &[u8] {
    let end = report
        .iter()
        .rposition(|&byte| byte != 0)
        .map_or(14, |index| (index + 1).max(14));
    &report[..end]
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

#[cfg(test)]
mod tests {
    use super::{BRIGHTNESS_STEP, adjust_brightness, visible_report};
    use mirabox::KnobDirection;

    #[test]
    fn brightness_steps_clamp_to_each_range() {
        for max in [60, 100] {
            assert_eq!(adjust_brightness(max - 2, KnobDirection::Clockwise, max), max);
            assert_eq!(adjust_brightness(max, KnobDirection::Clockwise, max), max);
            assert_eq!(adjust_brightness(2, KnobDirection::CounterClockwise, max), 0);
            assert_eq!(adjust_brightness(0, KnobDirection::CounterClockwise, max), 0);
            assert_eq!(
                adjust_brightness(max / 2, KnobDirection::Clockwise, max),
                max / 2 + BRIGHTNESS_STEP
            );
        }
    }

    #[test]
    fn raw_display_keeps_header_and_nonzero_unknown_fields() {
        let mut report = [0; 512];
        assert_eq!(visible_report(&report).len(), 14);
        report[20] = 0xaa;
        assert_eq!(visible_report(&report), &report[..21]);
    }
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
