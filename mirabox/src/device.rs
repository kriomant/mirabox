use std::{collections::HashMap, error::Error, fmt};

use async_hid::{AsyncHidRead, AsyncHidWrite, DeviceReader, DeviceWriter, HidBackend, HidError};
use futures_lite::StreamExt;
use image::{DynamicImage, ImageError, codecs::jpeg::JpegEncoder, imageops::FilterType};
use tokio::sync::Mutex;

use crate::{
    HID_USAGE_ID, HID_USAGE_PAGE, ImageFormat, MIRABOX_VENDOR_ID, N4_PRO_PRODUCT_ID, RawDeviceInput,
};

const PACKET_SIZE: usize = 1024;
const REPORT_SIZE: usize = PACKET_SIZE + 1;
const COMMAND_PREFIX: &[u8] = b"\0CRT\0\0";

#[derive(Debug)]
pub enum DeviceError {
    Hid(HidError),
    Image(ImageError),
    DeviceNotFound,
    MissingSerialNumber,
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hid(error) => write!(f, "HID error: {error}"),
            Self::Image(error) => write!(f, "image error: {error}"),
            Self::DeviceNotFound => write!(f, "device disappeared during connection"),
            Self::MissingSerialNumber => write!(f, "device has no serial number"),
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

struct Writer {
    device: DeviceWriter,
    initialized: bool,
}

pub(crate) struct Device {
    reader: Mutex<DeviceReader>,
    writer: Mutex<Writer>,
    images: Mutex<HashMap<u8, Vec<u8>>>,
    pub serial_number: String,
    pub firmware_version: Option<String>,
}

impl Device {
    pub async fn connect() -> Result<Option<Self>, DeviceError> {
        let backend = HidBackend::default();
        let Some(info) = backend
            .enumerate()
            .await?
            .find(|device| {
                device.matches(
                    HID_USAGE_PAGE,
                    HID_USAGE_ID,
                    MIRABOX_VENDOR_ID,
                    N4_PRO_PRODUCT_ID,
                )
            })
            .await
        else {
            return Ok(None);
        };

        let device = backend
            .query_devices(&info.id)
            .await?
            .last()
            .ok_or(DeviceError::DeviceNotFound)?;

        #[cfg(not(target_os = "windows"))]
        let firmware_version = {
            let mut buffer = [0; 20];
            buffer[0] = 1;
            let size = device.read_feature_report(&mut buffer).await?;
            Some(String::from_utf8_lossy(&buffer[..size]).into_owned())
        };
        #[cfg(target_os = "windows")]
        let firmware_version = None;

        let serial_number = device
            .serial_number
            .clone()
            .ok_or(DeviceError::MissingSerialNumber)?;
        let (reader, writer) = device.open().await?;

        Ok(Some(Self {
            reader: Mutex::new(reader),
            writer: Mutex::new(Writer {
                device: writer,
                initialized: false,
            }),
            images: Mutex::new(HashMap::new()),
            serial_number,
            firmware_version,
        }))
    }

    pub async fn set_image(
        &self,
        key: u8,
        format: ImageFormat,
        image: DynamicImage,
    ) -> Result<(), DeviceError> {
        self.writer.lock().await.initialize().await?;
        let bytes = tokio::task::block_in_place(move || encode_image(format, image))?;
        self.images.lock().await.insert(key, bytes);
        Ok(())
    }

    pub async fn flush(&self) -> Result<(), DeviceError> {
        let mut images = self.images.lock().await;
        let mut writer = self.writer.lock().await;
        writer.initialize().await?;
        if images.is_empty() {
            return Ok(());
        }

        for (key, bytes) in images.drain() {
            writer
                .write_command(&image_header(key, bytes.len()))
                .await?;
            for chunk in bytes.chunks(PACKET_SIZE) {
                let mut report = [0; REPORT_SIZE];
                report[1..1 + chunk.len()].copy_from_slice(chunk);
                writer.device.write_output_report(&report).await?;
            }
        }
        writer.write_command(b"STP").await?;
        Ok(())
    }

    pub async fn keep_alive(&self) -> Result<(), DeviceError> {
        let mut writer = self.writer.lock().await;
        writer.initialize().await?;
        writer.write_command(b"CONNECT").await
    }

    pub async fn read_raw_input(&self) -> Result<Option<RawDeviceInput>, DeviceError> {
        let mut buffer = [0; 512];
        let size = self
            .reader
            .lock()
            .await
            .read_input_report(&mut buffer)
            .await?;
        Ok(parse_input(&buffer[..size.min(buffer.len())]))
    }
}

impl Writer {
    async fn initialize(&mut self) -> Result<(), DeviceError> {
        if !self.initialized {
            self.write_command(b"DIS").await?;
            self.write_command(b"LIG\0\0\0\0").await?;
            self.initialized = true;
        }
        Ok(())
    }

    async fn write_command(&mut self, command: &[u8]) -> Result<(), DeviceError> {
        self.device
            .write_output_report(&command_report(command))
            .await?;
        Ok(())
    }
}

fn command_report(command: &[u8]) -> [u8; REPORT_SIZE] {
    let mut report = [0; REPORT_SIZE];
    report[..COMMAND_PREFIX.len()].copy_from_slice(COMMAND_PREFIX);
    report[COMMAND_PREFIX.len()..COMMAND_PREFIX.len() + command.len()].copy_from_slice(command);
    report
}

fn image_header(key: u8, length: usize) -> [u8; 8] {
    let [high, low] = (length as u16).to_be_bytes();
    [b'B', b'A', b'T', 0, 0, high, low, key + 1]
}

fn parse_input(report: &[u8]) -> Option<RawDeviceInput> {
    if report.len() < 11 || !report.starts_with(b"ACK") {
        return None;
    }
    Some(RawDeviceInput::new(report[9], report[10]))
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
    fn frames_image_upload_and_commit() {
        let header = command_report(&image_header(10, 0x1234));
        assert_eq!(&header[..14], b"\0CRT\0\0BAT\0\0\x12\x34\x0b");
        assert!(header[14..].iter().all(|byte| *byte == 0));
        let commit = command_report(b"STP");
        assert_eq!(&commit[..9], b"\0CRT\0\0STP");
        assert_eq!(commit.len(), 1025);
    }

    #[test]
    fn parses_only_complete_ack_reports() {
        assert_eq!(
            parse_input(b"ACK\0\0\0\0\0\0\xb1\xff"),
            Some(RawDeviceInput::new(177, 255))
        );
        assert_eq!(parse_input(b"ACK\0\0\0\0\0\0\xb1"), None);
        assert_eq!(parse_input(b"BAD\0\0\0\0\0\0\xb1\xff"), None);
    }
}
