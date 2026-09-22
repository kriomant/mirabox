//! Support for communicating with a MiraBox N4 Pro.

mod event;

use std::sync::{Arc, Mutex};

use arrayvec::ArrayVec;
use image::DynamicImage;
use mirajazz::{
    device::{Device, DeviceQuery, list_devices},
    error::MirajazzError,
    state::DeviceStateReader,
    types::{DeviceInput, ImageFormat, ImageMirroring, ImageMode, ImageRotation},
};

pub use event::{
    DeviceEvent, EnrichedDeviceEvent, EventParser, HorizontalDirection, KnobDirection,
    RawDeviceInput, SwipeDirection, VerticalDirection,
};

const HID_USAGE_PAGE: u16 = 0xffa0;
const HID_USAGE_ID: u16 = 1;
const MIRABOX_VENDOR_ID: u16 = 0x5548;
const N4_PRO_PRODUCT_ID: u16 = 0x1021;
const N4_PRO_PROTOCOL_VERSION: usize = 3;
const N4_PRO_BUTTON_COUNT: usize = 10;
const N4_PRO_VIRTUAL_BUTTON_COUNT: usize = 4;
const N4_PRO_KEY_COUNT: usize = N4_PRO_BUTTON_COUNT + N4_PRO_VIRTUAL_BUTTON_COUNT;
const N4_PRO_ENCODER_COUNT: usize = 3;
const N4_PRO_PHYSICAL_BUTTON_IMAGE_KEYS: [u8; N4_PRO_BUTTON_COUNT] =
    [10, 11, 12, 13, 14, 5, 6, 7, 8, 9];

/// Image format for a physical button display.
pub const PHYSICAL_BUTTON_IMAGE_FORMAT: ImageFormat = ImageFormat {
    mode: ImageMode::JPEG,
    size: (128, 128),
    rotation: ImageRotation::Rot180,
    mirror: ImageMirroring::None,
};

/// Image format for a touch-screen virtual button display.
pub const VIRTUAL_BUTTON_IMAGE_FORMAT: ImageFormat = ImageFormat {
    mode: ImageMode::JPEG,
    size: (203, 128),
    rotation: ImageRotation::Rot180,
    mirror: ImageMirroring::None,
};

/// A connected MiraBox N4 Pro.
pub struct MiraboxN4Pro {
    device: Device,
    reader: Arc<DeviceStateReader>,
    event_parser: Mutex<EventParser>,
}

impl MiraboxN4Pro {
    /// Connects to the first detected MiraBox N4 Pro, if one is available.
    pub async fn connect() -> Result<Option<Self>, MirajazzError> {
        let query = DeviceQuery::new(
            HID_USAGE_PAGE,
            HID_USAGE_ID,
            MIRABOX_VENDOR_ID,
            N4_PRO_PRODUCT_ID,
        );
        let Some(raw_device) = list_devices(&[query]).await?.into_iter().next() else {
            return Ok(None);
        };

        let device = Device::connect(
            &raw_device,
            N4_PRO_PROTOCOL_VERSION,
            N4_PRO_KEY_COUNT,
            N4_PRO_ENCODER_COUNT,
        )
        .await?;
        let reader = device.get_reader(encode_input);

        Ok(Some(Self {
            device,
            reader,
            event_parser: Mutex::new(EventParser::new()),
        }))
    }

    /// Returns the zero-based physical button's image key, if the button exists.
    pub const fn physical_button_image_key(button: u8) -> Option<u8> {
        if (button as usize) < N4_PRO_PHYSICAL_BUTTON_IMAGE_KEYS.len() {
            Some(N4_PRO_PHYSICAL_BUTTON_IMAGE_KEYS[button as usize])
        } else {
            None
        }
    }

    /// Queues an image for the display identified by `key`.
    ///
    /// Call [`Self::flush`] to make queued images visible.
    pub async fn set_image(
        &self,
        key: u8,
        format: ImageFormat,
        image: DynamicImage,
    ) -> Result<(), MirajazzError> {
        self.device.set_button_image(key, format, image).await
    }

    /// Commits queued images to the device.
    pub async fn flush(&self) -> Result<(), MirajazzError> {
        self.device.flush().await
    }

    /// Sends a keep-alive message to the device.
    pub async fn keep_alive(&self) -> Result<(), MirajazzError> {
        self.device.keep_alive().await
    }

    /// Waits for the next raw device input report.
    ///
    /// `None` denotes a transport report that did not contain input data.
    pub async fn read_raw_input(&self) -> Result<Option<RawDeviceInput>, MirajazzError> {
        let input = self.reader.read_input(None, encode_input).await?;
        Ok(RawDeviceInput::try_from(input).ok())
    }

    /// Waits for the next device input report and decodes it with inferred state.
    ///
    /// Returns zero, one, or two enriched events for the next input report.
    ///
    /// A short touch generates both a synthesized `Touch` and `TouchReleased`
    /// event. Reports without input or a recognized event return an empty list.
    pub async fn read_event(
        &self,
    ) -> Result<ArrayVec<EnrichedDeviceEvent, 2>, MirajazzError> {
        let Some(raw) = self.read_raw_input().await? else {
            return Ok(ArrayVec::new());
        };

        let Ok(event) = DeviceEvent::try_from(raw) else {
            return Ok(ArrayVec::new());
        };

        Ok(self
            .event_parser
            .lock()
            .expect("event parser mutex is only held while parsing a report")
            .parse(event))
    }

    /// Returns the device serial number.
    pub fn serial_number(&self) -> &str {
        self.device.serial_number()
    }

    /// Returns the device firmware version, when reported by the device.
    pub fn firmware_version(&self) -> Option<&str> {
        self.device.firmware_version.as_deref()
    }
}

fn encode_input(control: u8, state: u8) -> Result<DeviceInput, MirajazzError> {
    Ok(RawDeviceInput::new(control, state).into_device_input())
}
