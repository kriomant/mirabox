use std::{env, error::Error, process::ExitCode, time::Duration};

mod event;

use image::{DynamicImage, ImageBuffer, Rgb};
use mirajazz::{
    device::{Device, DeviceQuery, list_devices},
    error::MirajazzError,
    types::{DeviceInput, ImageFormat, ImageMirroring, ImageMode, ImageRotation},
};

use crate::event::{DeviceEvent, RawDeviceInput};

const HID_USAGE_PAGE: u16 = 0xffa0;
const HID_USAGE_ID: u16 = 1;
const MIRABOX_VENDOR_ID: u16 = 0x5548;
const N4_PRO_PRODUCT_ID: u16 = 0x1021;
const N4_PRO_PROTOCOL_VERSION: usize = 3;
const N4_PRO_BUTTON_COUNT: usize = 10;
const N4_PRO_VIRTUAL_BUTTON_COUNT: usize = 4;
const N4_PRO_KEY_COUNT: usize = N4_PRO_BUTTON_COUNT + N4_PRO_VIRTUAL_BUTTON_COUNT;
const N4_PRO_PHYSICAL_BUTTON_IMAGE_KEYS: [u8; N4_PRO_BUTTON_COUNT] =
    [10, 11, 12, 13, 14, 5, 6, 7, 8, 9];

const N4_PRO_ENCODER_COUNT: usize = 3;
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(1);

const BUTTON_IMAGE_FORMAT: ImageFormat = ImageFormat {
    mode: ImageMode::JPEG,
    size: (128, 128),
    rotation: ImageRotation::Rot180,
    mirror: ImageMirroring::None,
};

const VIRTUAL_BUTTON_IMAGE_FORMAT: ImageFormat = ImageFormat {
    mode: ImageMode::JPEG,
    size: (203, 128),
    rotation: ImageRotation::Rot180,
    mirror: ImageMirroring::None,
};

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
async fn main() -> Result<ExitCode, Box<dyn Error>> {
    let (vendor_id, product_id) = device_ids(env::args().skip(1))?;
    let query = DeviceQuery::new(HID_USAGE_PAGE, HID_USAGE_ID, vendor_id, product_id);
    let devices = list_devices(&[query]).await?;

    if devices.is_empty() {
        eprintln!(
            "No MiraBox N4 Pro found at {vendor_id:04x}:{product_id:04x}. Connect it, then run this program again."
        );
        return Ok(ExitCode::FAILURE);
    }

    let raw_device = devices
        .into_iter()
        .next()
        .expect("the device set was checked above");
    let firmware = Device::read_firmware_version(&raw_device).await?;
    let serial_number = raw_device.serial_number.as_deref().unwrap_or("unknown");

    let firmware = firmware
        .as_deref()
        .unwrap_or("unknown")
        .trim_end_matches('\0');

    println!(
        "Communication succeeded: {vendor_id:04x}:{product_id:04x}, serial {serial_number}, firmware {firmware}"
    );

    let device = Device::connect(
        &raw_device,
        N4_PRO_PROTOCOL_VERSION,
        N4_PRO_KEY_COUNT,
        N4_PRO_ENCODER_COUNT,
    )
    .await?;

    for virtual_button in 0..N4_PRO_VIRTUAL_BUTTON_COUNT as u8 {
        device
            .set_button_image(
                virtual_button,
                VIRTUAL_BUTTON_IMAGE_FORMAT,
                test_image(virtual_button, VIRTUAL_BUTTON_IMAGE_FORMAT.size),
            )
            .await?;
    }

    for (button, key) in N4_PRO_PHYSICAL_BUTTON_IMAGE_KEYS.into_iter().enumerate() {
        device
            .set_button_image(
                key,
                BUTTON_IMAGE_FORMAT,
                test_image(button as u8, BUTTON_IMAGE_FORMAT.size),
            )
            .await?;
    }
    device.flush().await?;

    println!(
        "Uploaded test images to {} button displays ({} physical, {} virtual).",
        device.key_count(),
        N4_PRO_BUTTON_COUNT,
        N4_PRO_VIRTUAL_BUTTON_COUNT
    );
    println!("Listening for button and touch events; press Control-C to exit.");

    let reader = device.get_reader(log_input);
    let mut keep_alive = tokio::time::interval(KEEP_ALIVE_INTERVAL);
    keep_alive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = keep_alive.tick() => device.keep_alive().await?,
            input = reader.read(None) => { input?; }
        }
    }
}

fn device_ids(arguments: impl Iterator<Item = String>) -> Result<(u16, u16), Box<dyn Error>> {
    let arguments = arguments.collect::<Vec<_>>();

    match arguments.as_slice() {
        [] => Ok((MIRABOX_VENDOR_ID, N4_PRO_PRODUCT_ID)),
        [vendor_id, product_id] => Ok((parse_hex_id(vendor_id)?, parse_hex_id(product_id)?)),
        _ => Err("usage: mirabox [vendor-id product-id] (hexadecimal, e.g. 5548 1021)".into()),
    }
}

fn log_input(control: u8, state: u8) -> Result<DeviceInput, MirajazzError> {
    let raw = RawDeviceInput::new(control, state);
    let input = raw.into_device_input();
    let decoded_raw = RawDeviceInput::try_from(input.clone())
        .expect("RawDeviceInput always encodes as a two-value EncoderTwist");
    let event = DeviceEvent::from(decoded_raw);

    println!("{event:?} (raw control {control}, state {state})");
    Ok(input)
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

fn parse_hex_id(value: &str) -> Result<u16, Box<dyn Error>> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    u16::from_str_radix(value, 16)
        .map_err(|_| format!("invalid hexadecimal USB identifier: {value}").into())
}
