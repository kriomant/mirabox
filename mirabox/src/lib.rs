//! Support for communicating with a MiraBox N4 Pro.

mod device;
mod event;

use std::{collections::HashMap, error::Error, fmt};

use async_hid::HidError;
use device::Device;
use tokio::sync::Mutex;

#[derive(Debug)]
pub enum DeviceError {
    Hid(HidError),
    DeviceNotFound,
    MissingSerialNumber,
    InvalidBrightness { value: u8, max: u8 },
    InvalidBackgroundSize { width: u32, height: u32 },
    BackgroundImageTooLarge { length: usize },
    ShortInputReport { expected: usize, actual: usize },
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hid(error) => write!(f, "HID error: {error}"),
            Self::DeviceNotFound => write!(f, "device disappeared during connection"),
            Self::MissingSerialNumber => write!(f, "device has no serial number"),
            Self::InvalidBrightness { value, max } => {
                write!(f, "brightness {value} exceeds the observed maximum of {max}")
            },
            Self::InvalidBackgroundSize { width, height } => {
                write!(f, "background dimensions {width}×{height} must fit in nonzero 16-bit fields")
            },
            Self::BackgroundImageTooLarge { length } => {
                write!(f, "background image length {length} exceeds the 32-bit protocol field")
            },
            Self::ShortInputReport { expected, actual } => {
                write!(f, "short input report: expected at least {expected} bytes, got {actual}")
            }
        }
    }
}

impl Error for DeviceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Hid(error) => Some(error),
            _ => None,
        }
    }
}

impl From<HidError> for DeviceError {
    fn from(error: HidError) -> Self {
        Self::Hid(error)
    }
}


/// Image dimensions for an N4 Pro display. Button images must be rotated 180° before upload.
#[derive(Clone, Copy, Debug)]
pub struct ImageFormat {
    /// Output width and height in pixels.
    pub size: (usize, usize),
}

pub use event::{DeviceEvent, KnobDirection, SwipeDirection};

const HID_USAGE_PAGE: u16 = 0xffa0;
const HID_USAGE_ID: u16 = 1;
const MIRABOX_VENDOR_ID: u16 = 0x5548;
const N4_PRO_PRODUCT_ID: u16 = 0x1021;
const N4_PRO_BUTTON_COUNT: usize = 10;
const N4_PRO_PHYSICAL_BUTTON_IMAGE_KEYS: [u8; N4_PRO_BUTTON_COUNT] =
    [10, 11, 12, 13, 14, 5, 6, 7, 8, 9];

/// Image format for a physical button display.
pub const PHYSICAL_BUTTON_IMAGE_FORMAT: ImageFormat = ImageFormat { size: (112, 112) };

/// Image format for a touch-screen virtual button display.
pub const VIRTUAL_BUTTON_IMAGE_FORMAT: ImageFormat = ImageFormat { size: (176, 112) };

/// A connected MiraBox N4 Pro.
pub struct MiraboxN4Pro {
    device: Device,
    images: Mutex<HashMap<u8, Vec<u8>>>,

}

impl MiraboxN4Pro {
    /// Connects to the first detected MiraBox N4 Pro, if one is available.
    pub async fn connect() -> Result<Option<Self>, DeviceError> {
        Ok(Device::connect().await?.map(|device| Self {
            device,
            images: Mutex::new(HashMap::new()),

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

    /// Queues encoded image bytes (such as JPEG or PNG) for the display identified by `key`.
    ///
    /// Prepare the image at the appropriate display size, rotated 180° before encoding.
    /// The bytes are uploaded unchanged.
    /// Call [`Self::flush`] to make queued images visible.
    pub async fn set_image(&self, key: u8, image: &[u8]) -> Result<(), DeviceError> {
        self.device.initialize().await?;
        self.images.lock().await.insert(key, image.to_vec());
        Ok(())
    }

    /// Uploads an encoded rectangular background image (such as JPEG or PNG)
    /// at `(x, y)` using the supplied dimensions and opaque, capture-specific
    /// selector bytes. The bytes are sent unchanged. Captured Button Area frames
    /// used `(2, 187, 798, 298)` and `[1, 0]`; Knob Area frames used
    /// `(2, 2, 798, 148)` and `[2, 0]`.
    /// An Overall Area frame also used `[1, 0]` with a larger rectangle, so
    /// the selector alone does not determine the area.
    /// Unlike button images, the upload is sent immediately without `flush`.
    pub async fn set_background_image(
        &self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        selector: [u8; 2],
        image: &[u8],
    ) -> Result<(), DeviceError> {
        if width == 0 || height == 0 {
            return Err(DeviceError::InvalidBackgroundSize {
                width: width.into(),
                height: height.into(),
            });
        }
        self.device
            .write_background_image(x, y, width, height, selector, image)
            .await
    }

    /// Resets the background to black with the captured `BGCLE 03 00` command.
    /// This is not a general-purpose clear operation for individual layers.
    pub async fn clear_background_image(&self) -> Result<(), DeviceError> {
        self.device.clear_background_image().await
    }

    /// Commits queued images to the device.
    pub async fn flush(&self) -> Result<(), DeviceError> {
        let mut images = self.images.lock().await;
        // Retain the entire batch for retry if any write or the commit fails.
        let pending: Vec<_> = images
            .iter()
            .map(|(&key, bytes)| (key, bytes.as_slice()))
            .collect();
        self.device.write_images(pending).await?;
        images.clear();
        Ok(())
    }

    /// Sets screen brightness on the captured 0–100 scale.
    /// Values outside this observed UI range are not sent to the device.
    pub async fn set_screen_brightness(&self, brightness: u8) -> Result<(), DeviceError> {
        validate_brightness(brightness, 100)?;
        self.device.set_screen_brightness(brightness).await
    }

    /// Sets dial/light brightness on the captured 0–60 scale.
    /// Values outside this observed UI range are not sent to the device.
    pub async fn set_light_brightness(&self, brightness: u8) -> Result<(), DeviceError> {
        validate_brightness(brightness, 60)?;
        self.device.set_light_brightness(brightness).await
    }

    /// Sets the RGB color of each knob's LED, in zero-based knob order (0–3).
    /// All four colors are sent together; each color is `[red, green, blue]`.
    pub async fn set_led_colors(&self, colors: [[u8; 3]; 4]) -> Result<(), DeviceError> {
        self.device.set_led_colors(colors).await
    }

    /// Enables or disables the device's vibro feedback.
    ///
    /// Uses a captured `QUCMD` template with “Follow the software” lighting mode;
    /// setting feedback also selects that mode. This command has not been
    /// independently replay-verified on hardware.
    pub async fn set_vibro_feedback(&self, enabled: bool) -> Result<(), DeviceError> {
        self.device.set_vibro_feedback(enabled).await
    }

    /// Sends a keep-alive message to the device.
    pub async fn keep_alive(&self) -> Result<(), DeviceError> {
        self.device.keep_alive().await
    }

    /// Reads a vendor input report and returns its first `N` bytes as an array.
    ///
    /// Use `N = 11` for `ACK/OK` code/state, `N = 14` for `ACK/ARX` X/Y,
    /// or `N = 512` to retain the entire report including unknown fields.
    /// Panics if `N > 512`; returns an error if the transport supplies fewer
    /// than `N` bytes. No heap allocation is performed for the returned array.
    pub async fn read_raw_report<const N: usize>(&self) -> Result<[u8; N], DeviceError> {
        self.device.read_raw_report().await
    }


    /// Waits for a report and returns its raw bytes alongside any decoded event.
    /// Unknown reports remain available even when they cannot be decoded.
    pub async fn read_report_event(&self) -> Result<([u8; 512], Option<DeviceEvent>), DeviceError> {
        let report = self.read_raw_report::<512>().await?;
        let event = DeviceEvent::decode(&report);
        Ok((report, event))
    }

    /// Waits for the next report and decodes it, returning `None` for unknown
    /// reports. Use [`Self::read_report_event`] to retain their raw bytes.
    pub async fn read_event(&self) -> Result<Option<DeviceEvent>, DeviceError> {
        Ok(self.read_report_event().await?.1)
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

fn validate_brightness(value: u8, max: u8) -> Result<(), DeviceError> {
    if value > max {
        Err(DeviceError::InvalidBrightness { value, max })
    } else {
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_stays_within_observed_ranges() {
        for max in [60, 100] {
            assert!(validate_brightness(0, max).is_ok());
            assert!(validate_brightness(max, max).is_ok());
            assert!(matches!(
                validate_brightness(max + 1, max),
                Err(DeviceError::InvalidBrightness { value, max: limit })
                    if value == max + 1 && limit == max
            ));
        }
    }

    #[test]
    fn default_image_formats_match_captured_dimensions() {
        assert_eq!(PHYSICAL_BUTTON_IMAGE_FORMAT.size, (112, 112));
        assert_eq!(VIRTUAL_BUTTON_IMAGE_FORMAT.size, (176, 112));
    }
}
