//! Support for communicating with a MiraBox N4 Pro.

mod device;
mod event;

use device::Device;
use image::DynamicImage;

pub use device::DeviceError;

/// JPEG image size for an N4 Pro display. Images are rotated 180° before upload.
#[derive(Clone, Copy, Debug)]
pub struct ImageFormat {
    /// Output width and height in pixels.
    pub size: (usize, usize),
}

pub use event::{
    DeviceEvent, EnrichedDeviceEvent, EventParser, KnobDirection, RawDeviceInput, SwipeDirection,
};

const HID_USAGE_PAGE: u16 = 0xffa0;
const HID_USAGE_ID: u16 = 1;
const MIRABOX_VENDOR_ID: u16 = 0x5548;
const N4_PRO_PRODUCT_ID: u16 = 0x1021;
const N4_PRO_BUTTON_COUNT: usize = 10;
const N4_PRO_PHYSICAL_BUTTON_IMAGE_KEYS: [u8; N4_PRO_BUTTON_COUNT] =
    [10, 11, 12, 13, 14, 5, 6, 7, 8, 9];

/// Image format for a physical button display.
pub const PHYSICAL_BUTTON_IMAGE_FORMAT: ImageFormat = ImageFormat { size: (128, 128) };

/// Image format for a touch-screen virtual button display.
pub const VIRTUAL_BUTTON_IMAGE_FORMAT: ImageFormat = ImageFormat { size: (203, 128) };

/// A connected MiraBox N4 Pro.
pub struct MiraboxN4Pro {
    device: Device,
    event_parser: EventParser,
}

impl MiraboxN4Pro {
    /// Connects to the first detected MiraBox N4 Pro, if one is available.
    pub async fn connect() -> Result<Option<Self>, DeviceError> {
        Ok(Device::connect().await?.map(|device| Self {
            device,
            event_parser: EventParser::new(),
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
    ) -> Result<(), DeviceError> {
        self.device.set_image(key, format, image).await
    }

    /// Commits queued images to the device.
    pub async fn flush(&self) -> Result<(), DeviceError> {
        self.device.flush().await
    }

    /// Sends a keep-alive message to the device.
    pub async fn keep_alive(&self) -> Result<(), DeviceError> {
        self.device.keep_alive().await
    }

    /// Waits for the next raw device input report.
    ///
    /// `None` denotes a transport report that did not contain input data.
    pub async fn read_raw_input(&self) -> Result<Option<RawDeviceInput>, DeviceError> {
        self.device.read_raw_input().await
    }

    /// Waits for the next device input report and decodes it.
    ///
    /// Returns an enriched event for the next input report, if there is one.
    ///
    /// Touch samples are ignored because their values identify touch-bar regions,
    /// not virtual buttons. A touch-release report produces an enriched `Touch`
    /// press event using its virtual-button index. Swipes are returned with only
    /// their direction. Reports without input or a recognized event return `None`.
    pub async fn read_event(&self) -> Result<Option<EnrichedDeviceEvent>, DeviceError> {
        let Some(raw) = self.read_raw_input().await? else {
            return Ok(None);
        };

        let Ok(event) = DeviceEvent::try_from(raw) else {
            return Ok(None);
        };

        Ok(self.event_parser.parse(event))
    }

    /// Returns the device serial number.
    pub fn serial_number(&self) -> &str {
        &self.device.serial_number
    }

    /// Returns the device firmware version, when reported by the device.
    pub fn firmware_version(&self) -> Option<&str> {
        self.device.firmware_version.as_deref()
    }
}
