use std::{collections::BTreeMap, error::Error, process::ExitCode, time::Duration};

mod event;
mod widget;

use clap::{Args, Parser, Subcommand};
use image::{DynamicImage, ImageBuffer, Rgb};
use mirajazz::{
    device::{Device, DeviceQuery, list_devices},
    error::MirajazzError,
    types::{DeviceInput, ImageFormat, ImageMirroring, ImageMode, ImageRotation},
};

use crate::{
    event::{DeviceEvent, RawDeviceInput},
    widget::{CounterWidget, Updated, Widget, WidgetEvent},
};

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

#[derive(Parser)]
#[command(about = "Communicate with a MiraBox N4 Pro")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Upload test images and log input events.
    Test(DeviceIds),
    /// Send periodic keep-alive messages.
    Run(DeviceIds),
}

#[derive(Args)]
struct DeviceIds {
    /// USB vendor ID, in hexadecimal.
    #[arg(value_parser = parse_hex_id)]
    vendor_id: Option<u16>,

    /// USB product ID, in hexadecimal.
    #[arg(value_parser = parse_hex_id, requires = "vendor_id")]
    product_id: Option<u16>,
}

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
    let cli = Cli::parse();
    let (command, device_ids) = match cli.command {
        Command::Test(device_ids) => (CommandKind::Test, device_ids),
        Command::Run(device_ids) => (CommandKind::Run, device_ids),
    };
    let (vendor_id, product_id) = device_ids.resolve()?;
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

    match command {
        CommandKind::Test => Ok(test(&device).await?),
        CommandKind::Run => Ok(run(&device).await?),
    }
}

#[derive(Clone, Copy)]
enum CommandKind {
    Run,
    Test,
}

struct WidgetImage {
    key: u8,
    format: ImageFormat,
    image: DynamicImage,
}

struct WidgetRegistry {
    widgets: BTreeMap<u8, Box<dyn Widget>>,
    update_sender: tokio::sync::mpsc::UnboundedSender<u8>,
}

impl WidgetRegistry {
    fn new(update_sender: tokio::sync::mpsc::UnboundedSender<u8>) -> Self {
        Self {
            widgets: BTreeMap::new(),
            update_sender,
        }
    }

    fn register(
        &mut self,
        button: u8,
        make_widget: impl FnOnce(Updated) -> Box<dyn Widget>,
    ) {
        assert!(
            (button as usize) < N4_PRO_PHYSICAL_BUTTON_IMAGE_KEYS.len(),
            "widget binding must use a valid physical button number"
        );

        let update_sender = self.update_sender.clone();
        let updated = Box::new(move || {
            let _ = update_sender.send(button);
        });
        assert!(
            self.widgets.insert(button, make_widget(updated)).is_none(),
            "a widget is already registered for button {button}"
        );
    }

    fn images(&self) -> Vec<WidgetImage> {
        self.widgets
            .iter()
            .map(|(&button, widget)| Self::image_for(button, widget.as_ref()))
            .collect()
    }

    fn handle_event(&mut self, button: u8, event: WidgetEvent) {
        if let Some(widget) = self.widgets.get_mut(&button) {
            widget.handle_event(event);
        }
    }

    fn image(&self, button: u8) -> Option<WidgetImage> {
        self.widgets
            .get(&button)
            .map(|widget| Self::image_for(button, widget.as_ref()))
    }

    fn image_for(button: u8, widget: &dyn Widget) -> WidgetImage {
        let key = N4_PRO_PHYSICAL_BUTTON_IMAGE_KEYS
            .get(button as usize)
            .copied()
            .expect("widget binding must use a valid physical button number");

        WidgetImage {
            key,
            format: BUTTON_IMAGE_FORMAT,
            image: widget.image(BUTTON_IMAGE_FORMAT.size),
        }
    }
}

impl DeviceIds {
    fn resolve(self) -> Result<(u16, u16), Box<dyn Error>> {
        match (self.vendor_id, self.product_id) {
            (None, None) => Ok((MIRABOX_VENDOR_ID, N4_PRO_PRODUCT_ID)),
            (Some(vendor_id), Some(product_id)) => Ok((vendor_id, product_id)),
            _ => Err("vendor ID and product ID must be provided together".into()),
        }
    }
}

async fn test(device: &Device) -> Result<ExitCode, MirajazzError> {
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

    let reader = device.get_reader(encode_input);
    let mut keep_alive = tokio::time::interval(KEEP_ALIVE_INTERVAL);
    keep_alive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = keep_alive.tick() => device.keep_alive().await?,
            input = reader.read_input(None, encode_input) => {
                let input = input?;
                let raw = RawDeviceInput::try_from(input)
                    .expect("encode_input always returns a two-value EncoderTwist");
                let event = DeviceEvent::from(raw);
                println!("{event:?} (raw control {}, state {})", raw.control, raw.state);
            }
        }
    }
}

async fn run(device: &Device) -> Result<ExitCode, MirajazzError> {
    let (widget_update_sender, mut widget_update_receiver) =
        tokio::sync::mpsc::unbounded_channel();
    let mut widgets = WidgetRegistry::new(widget_update_sender);
    widgets.register(0, |updated| Box::new(CounterWidget::new(updated)));

    for image in widgets.images() {
        device.set_button_image(image.key, image.format, image.image).await?;
    }
    device.flush().await?;

    println!("Running widgets; press Control-C to exit.");

    let reader = device.get_reader(encode_input);
    let mut keep_alive = tokio::time::interval(KEEP_ALIVE_INTERVAL);
    keep_alive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = keep_alive.tick() => device.keep_alive().await?,
            Some(button) = widget_update_receiver.recv() => {
                if let Some(image) = widgets.image(button) {
                    device.set_button_image(image.key, image.format, image.image).await?;
                    device.flush().await?;
                }
            }
            input = reader.read_input(None, encode_input) => {
                let input = input?;
                let raw = RawDeviceInput::try_from(input)
                    .expect("encode_input always returns a two-value EncoderTwist");
                if let DeviceEvent::Button { button, pressed } = DeviceEvent::from(raw) {
                    let widget_event = if pressed {
                        WidgetEvent::Pressed
                    } else {
                        WidgetEvent::Released
                    };
                    widgets.handle_event(button, widget_event);
                }
            }
        }
    }
}

fn encode_input(control: u8, state: u8) -> Result<DeviceInput, MirajazzError> {
    Ok(RawDeviceInput::new(control, state).into_device_input())
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

fn parse_hex_id(value: &str) -> Result<u16, String> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    u16::from_str_radix(value, 16)
        .map_err(|_| format!("invalid hexadecimal USB identifier: {value}"))
}
