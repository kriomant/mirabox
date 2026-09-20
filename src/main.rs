use std::{env, error::Error, process::ExitCode};

use mirajazz::device::{Device, DeviceQuery, list_devices};

const HID_USAGE_PAGE: u16 = 0xffa0;
const HID_USAGE_ID: u16 = 1;
const MIRABOX_VENDOR_ID: u16 = 0x5548;
const N4_PRO_PRODUCT_ID: u16 = 0x1021;

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

    for raw_device in devices {
        let firmware = Device::read_firmware_version(&raw_device).await?;
        let serial_number = raw_device.serial_number.as_deref().unwrap_or("unknown");

        let firmware = firmware.as_deref().unwrap_or("unknown").trim_end_matches('\0');

        println!(
            "Communication succeeded: {vendor_id:04x}:{product_id:04x}, serial {serial_number}, firmware {firmware}"
        );
    }

    Ok(ExitCode::SUCCESS)
}

fn device_ids(arguments: impl Iterator<Item = String>) -> Result<(u16, u16), Box<dyn Error>> {
    let arguments = arguments.collect::<Vec<_>>();

    match arguments.as_slice() {
        [] => Ok((MIRABOX_VENDOR_ID, N4_PRO_PRODUCT_ID)),
        [vendor_id, product_id] => Ok((parse_hex_id(vendor_id)?, parse_hex_id(product_id)?)),
        _ => Err("usage: mirabox [vendor-id product-id] (hexadecimal, e.g. 5548 1021)".into()),
    }
}

fn parse_hex_id(value: &str) -> Result<u16, Box<dyn Error>> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    u16::from_str_radix(value, 16)
        .map_err(|_| format!("invalid hexadecimal USB identifier: {value}").into())
}
