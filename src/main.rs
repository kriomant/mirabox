use std::{env, error::Error, process::ExitCode, time::Duration};

use image::{DynamicImage, ImageBuffer, Rgb};
use mirajazz::{
    device::{Device, DeviceQuery, list_devices},
    error::MirajazzError,
    types::{DeviceInput, ImageFormat, ImageMirroring, ImageMode, ImageRotation},
};

const HID_USAGE_PAGE: u16 = 0xffa0;
const HID_USAGE_ID: u16 = 1;
const MIRABOX_VENDOR_ID: u16 = 0x5548;
const N4_PRO_PRODUCT_ID: u16 = 0x1021;
const N4_PRO_PROTOCOL_VERSION: usize = 3;
const N4_PRO_KEY_COUNT: usize = 14;

const N4_PRO_ENCODER_COUNT: usize = 3;
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(1);

const BUTTON_IMAGE_FORMAT: ImageFormat = ImageFormat {
    mode: ImageMode::JPEG,
    size: (96, 96),
    rotation: ImageRotation::Rot0,
    mirror: ImageMirroring::None,
};

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

    for key in 0..N4_PRO_KEY_COUNT as u8 {
        device
            .set_button_image(key, BUTTON_IMAGE_FORMAT, test_image(key))
            .await?;
    }
    device.flush().await?;

    println!(
        "Uploaded test images to {} button displays.",
        device.key_count()
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
    let action = match state {
        0 => "released",
        _ => "pressed",
    };

    println!("Control {control}: {action} (raw state {state})");
    Ok(DeviceInput::NoData)
}

fn test_image(key: u8) -> DynamicImage {
    let color = [
        key.wrapping_mul(47).wrapping_add(40),
        key.wrapping_mul(83).wrapping_add(90),
        key.wrapping_mul(131).wrapping_add(140),
    ];

    let image = ImageBuffer::from_fn(96, 96, |x, y| {
        if x < 6 || y < 6 || x >= 90 || y >= 90 {
            Rgb([255, 255, 255])
        } else {
            Rgb(color)
        }
    });

    DynamicImage::ImageRgb8(image)
}

fn parse_hex_id(value: &str) -> Result<u16, Box<dyn Error>> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    u16::from_str_radix(value, 16)
        .map_err(|_| format!("invalid hexadecimal USB identifier: {value}").into())
}
