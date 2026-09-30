use async_hid::{AsyncHidRead, AsyncHidWrite, DeviceReader, DeviceWriter, HidBackend};
use futures_lite::StreamExt;

use tokio::sync::Mutex;

use crate::{DeviceError, HID_USAGE_ID, HID_USAGE_PAGE, MIRABOX_VENDOR_ID, N4_PRO_PRODUCT_ID};

const PACKET_SIZE: usize = 1024;
const REPORT_SIZE: usize = PACKET_SIZE + 1;
const COMMAND_PREFIX: &[u8] = b"\0CRT\0\0";

struct Writer {
    device: DeviceWriter,
    initialized: bool,
}

pub(crate) struct Device {
    reader: Mutex<DeviceReader>,
    writer: Mutex<Writer>,
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
            serial_number,
            firmware_version,
        }))
    }

    pub async fn initialize(&self) -> Result<(), DeviceError> {
        self.writer.lock().await.initialize().await
    }

    pub async fn write_images<'a>(
        &self,
        images: impl IntoIterator<Item = (u8, &'a [u8])>,
    ) -> Result<(), DeviceError> {
        let mut writer = self.writer.lock().await;
        writer.initialize().await?;
        let mut wrote_image = false;

        for (key, bytes) in images {
            wrote_image = true;
            writer
                .write_command(&image_header(key, bytes.len()))
                .await?;
            for chunk in bytes.chunks(PACKET_SIZE) {
                let mut report = [0; REPORT_SIZE];
                report[1..1 + chunk.len()].copy_from_slice(chunk);
                writer.device.write_output_report(&report).await?;
            }
        }
        if wrote_image {
            writer.write_command(b"STP").await?;
        }
        Ok(())
    }

    pub async fn set_screen_brightness(&self, brightness: u8) -> Result<(), DeviceError> {
        let mut writer = self.writer.lock().await;
        writer.initialize().await?;
        writer
            .write_command(&brightness_command(b"LIG\0\0", brightness))
            .await
    }

    pub async fn set_light_brightness(&self, brightness: u8) -> Result<(), DeviceError> {
        let mut writer = self.writer.lock().await;
        writer.initialize().await?;
        writer
            .write_command(&brightness_command(b"LBLIG", brightness))
            .await
    }

    pub async fn keep_alive(&self) -> Result<(), DeviceError> {
        let mut writer = self.writer.lock().await;
        writer.initialize().await?;
        writer.write_command(b"CONNECT").await
    }

    pub async fn read_raw_report<const N: usize>(&self) -> Result<[u8; N], DeviceError> {
        assert!(N <= 512, "vendor input report has at most 512 bytes");
        let mut buffer = [0; 512];
        let size = self
            .reader
            .lock()
            .await
            .read_input_report(&mut buffer)
            .await?;
        report_prefix(&buffer[..size.min(buffer.len())])
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

fn brightness_command(name: &[u8; 5], brightness: u8) -> [u8; 6] {
    let mut command = [0; 6];
    command[..5].copy_from_slice(name);
    command[5] = brightness;
    command
}

fn image_header(key: u8, length: usize) -> [u8; 8] {
    let [high, low] = (length as u16).to_be_bytes();
    [b'B', b'A', b'T', 0, 0, high, low, key + 1]
}

fn report_prefix<const N: usize>(bytes: &[u8]) -> Result<[u8; N], DeviceError> {
    if bytes.len() < N {
        return Err(DeviceError::ShortInputReport {
            expected: N,
            actual: bytes.len(),
        });
    }
    let mut prefix = [0; N];
    prefix.copy_from_slice(&bytes[..N]);
    Ok(prefix)
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
    fn frames_screen_and_light_brightness() {
        for (name, value, expected) in [
            (b"LIG\0\0", 100, b"\0CRT\0\0LIG\0\0\x64".as_slice()),
            (b"LBLIG", 60, b"\0CRT\0\0LBLIG\x3c".as_slice()),
        ] {
            let report = command_report(&brightness_command(name, value));
            assert_eq!(&report[..12], expected);
            assert!(report[12..].iter().all(|byte| *byte == 0));
            assert_eq!(report.len(), REPORT_SIZE);
        }
        let screen_off = command_report(&brightness_command(b"LIG\0\0", 0));
        let lights_off = command_report(&brightness_command(b"LBLIG", 0));
        assert_eq!(screen_off[11], 0);
        assert_eq!(lights_off[11], 0);
    }

    #[test]
    fn copies_only_requested_prefix_and_rejects_short_reports() {
        let report = [1, 2, 3, 4];
        assert_eq!(report_prefix::<2>(&report).unwrap(), [1, 2]);
        assert!(matches!(
            report_prefix::<5>(&report),
            Err(DeviceError::ShortInputReport {
                expected: 5,
                actual: 4
            })
        ));
    }
}
