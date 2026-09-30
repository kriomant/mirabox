//! Support for communicating with a MiraBox N4 Pro.

mod device;
mod event;

use std::{collections::HashMap, error::Error, fmt};

use async_hid::HidError;
use device::Device;
use image::{DynamicImage, ImageError, codecs::jpeg::JpegEncoder, imageops::FilterType};
use tokio::sync::Mutex;

#[derive(Debug)]
pub enum DeviceError {
    Hid(HidError),
    Image(ImageError),
    DeviceNotFound,
    MissingSerialNumber,
    InvalidBrightness { value: u8, max: u8 },
    ShortInputReport { expected: usize, actual: usize },
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hid(error) => write!(f, "HID error: {error}"),
            Self::Image(error) => write!(f, "image error: {error}"),
            Self::DeviceNotFound => write!(f, "device disappeared during connection"),
            Self::MissingSerialNumber => write!(f, "device has no serial number"),
            Self::InvalidBrightness { value, max } => {
                write!(f, "brightness {value} exceeds the observed maximum of {max}")
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
            Self::Image(error) => Some(error),
            _ => None,
        }
    }
}

impl From<HidError> for DeviceError {
    fn from(error: HidError) -> Self {
        Self::Hid(error)
    }
}

impl From<ImageError> for DeviceError {
    fn from(error: ImageError) -> Self {
        Self::Image(error)
    }
}

/// JPEG image size for an N4 Pro display. Images are rotated 180° before upload.
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

    /// Queues an image for the display identified by `key`.
    ///
    /// Call [`Self::flush`] to make queued images visible.
    pub async fn set_image(
        &self,
        key: u8,
        format: ImageFormat,
        image: DynamicImage,
    ) -> Result<(), DeviceError> {
        self.device.initialize().await?;
        let bytes = tokio::task::block_in_place(move || encode_image(format, image))?;
        self.images.lock().await.insert(key, bytes);
        Ok(())
    }

    /// Commits queued images to the device.
    pub async fn flush(&self) -> Result<(), DeviceError> {
        let mut images = self.images.lock().await;
        // Retain the entire batch for retry if any write or the commit fails.
        self.device
            .write_images(images.iter().map(|(&key, bytes)| (key, bytes.as_slice())))
            .await?;
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

fn encode_image(format: ImageFormat, image: DynamicImage) -> Result<Vec<u8>, ImageError> {
    let (width, height) = format.size;
    let image = image
        .resize_exact(width as u32, height as u32, FilterType::Lanczos3)
        .rotate180()
        .into_rgb8();
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 90).encode(
        image.as_raw(),
        width as u32,
        height as u32,
        image::ColorType::Rgb8.into(),
    )?;
    Ok(bytes)
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

    #[test]
    fn encodes_rotated_jpeg_at_requested_size() {
        let mut image = image::RgbImage::new(2, 1);
        image.put_pixel(0, 0, image::Rgb([255, 0, 0]));
        image.put_pixel(1, 0, image::Rgb([0, 0, 255]));

        let bytes = encode_image(ImageFormat { size: (2, 1) }, DynamicImage::ImageRgb8(image))
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().into_rgb8();
        assert_eq!(decoded.dimensions(), (2, 1));
        assert!(decoded.get_pixel(0, 0)[2] > decoded.get_pixel(0, 0)[0]);
        assert!(decoded.get_pixel(1, 0)[0] > decoded.get_pixel(1, 0)[2]);
    }
}
