use async_hid::{AsyncHidRead, AsyncHidWrite, DeviceReader, DeviceWriter, HidBackend};
use futures_lite::StreamExt;

use tokio::sync::Mutex;

use crate::{DeviceError, HID_USAGE_ID, HID_USAGE_PAGE, MIRABOX_VENDOR_ID, N4_PRO_PRODUCT_ID};

const PACKET_SIZE: usize = 1024;
const REPORT_SIZE: usize = PACKET_SIZE + 1;
const COMMAND_PREFIX: &[u8] = b"\0CRT\0\0";
const VIBRO_FEEDBACK_COMMAND: [u8; 11] = *b"QUCMD\x1f\x11\0\x11\0\x11";

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
            writer.write_image_data(bytes).await?;
        }
        if wrote_image {
            writer.write_command(b"STP").await?;
        }
        Ok(())
    }

    pub async fn write_background_image(
        &self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        selector: [u8; 2],
        bytes: &[u8],
    ) -> Result<(), DeviceError> {
        let length = u32::try_from(bytes.len())
            .map_err(|_| DeviceError::BackgroundImageTooLarge { length: bytes.len() })?;
        let mut writer = self.writer.lock().await;
        writer.initialize().await?;
        writer
            .write_command(&background_image_header(length, x, y, width, height, selector))
            .await?;
        writer.write_image_data(bytes).await
    }

    pub async fn clear_background_image(&self) -> Result<(), DeviceError> {
        let mut writer = self.writer.lock().await;
        writer.initialize().await?;
        writer.write_command(b"BGCLE\x03\0").await
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

    pub async fn set_led_colors(&self, colors: [[u8; 3]; 4]) -> Result<(), DeviceError> {
        let mut writer = self.writer.lock().await;
        writer.initialize().await?;
        writer.write_command(&led_colors_command(colors)).await
    }

    pub async fn set_vibro_feedback(&self, enabled: bool) -> Result<(), DeviceError> {
        let mut writer = self.writer.lock().await;
        writer.initialize().await?;
        writer.write_command(&vibro_feedback_command(enabled)).await
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

    async fn write_image_data(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        for chunk in bytes.chunks(PACKET_SIZE) {
            let mut report = [0; REPORT_SIZE];
            report[1..1 + chunk.len()].copy_from_slice(chunk);
            self.device.write_output_report(&report).await?;
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

fn led_colors_command(colors: [[u8; 3]; 4]) -> [u8; 17] {
    let mut command = [0; 17];
    command[..5].copy_from_slice(b"SETLB");
    for (index, color) in colors.into_iter().enumerate() {
        command[5 + index * 3..8 + index * 3].copy_from_slice(&color);
    }
    command
}

fn vibro_feedback_command(enabled: bool) -> [u8; 11] {
    let mut command = VIBRO_FEEDBACK_COMMAND;
    command[8] = if enabled { 0x11 } else { 0xff };
    command
}

fn image_header(key: u8, length: usize) -> [u8; 8] {
    let [high, low] = (length as u16).to_be_bytes();
    [b'B', b'A', b'T', 0, 0, high, low, key + 1]
}

fn background_image_header(
    length: u32,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    selector: [u8; 2],
) -> [u8; 19] {
    let mut command = [0; 19];
    command[..5].copy_from_slice(b"BGPIC");
    command[5..9].copy_from_slice(&length.to_be_bytes());
    for (offset, value) in [(9, x), (11, y), (13, width), (15, height)] {
        command[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }
    command[17..19].copy_from_slice(&selector);
    command
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
    fn frames_background_upload_and_clear() {
        let header = command_report(&background_image_header(
            22_517, 1, 1, 798, 478, [1, 0],
        ));
        assert_eq!(
            &header[..25],
            b"\0CRT\0\0BGPIC\0\0\x57\xf5\0\x01\0\x01\x03\x1e\x01\xde\x01\0"
        );
        assert!(header[25..].iter().all(|byte| *byte == 0));
        let button_area = command_report(&background_image_header(
            9_383, 2, 187, 798, 298, [1, 0],
        ));
        assert_eq!(
            &button_area[6..25],
            b"BGPIC\0\0\x24\xa7\0\x02\0\xbb\x03\x1e\x01\x2a\x01\0"
        );
        let knob_area = command_report(&background_image_header(
            6_500, 2, 2, 798, 148, [2, 0],
        ));
        assert_eq!(
            &knob_area[6..25],
            b"BGPIC\0\0\x19\x64\0\x02\0\x02\x03\x1e\0\x94\x02\0"
        );
        let clear = command_report(b"BGCLE\x03\0");
        assert_eq!(&clear[..13], b"\0CRT\0\0BGCLE\x03\0");
        assert!(clear[13..].iter().all(|byte| *byte == 0));
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
    fn frames_per_knob_led_colors() {
        let report = command_report(&led_colors_command([
            [0xff, 0, 0],
            [0, 0x80, 0],
            [0, 0, 0xff],
            [0x12, 0x34, 0x56],
        ]));
        assert_eq!(
            &report[..23],
            b"\0CRT\0\0SETLB\xff\0\0\0\x80\0\0\0\xff\x12\x34\x56"
        );
        assert!(report[23..].iter().all(|byte| *byte == 0));
        assert_eq!(report.len(), REPORT_SIZE);
    }

    #[test]
    fn frames_vibro_feedback_toggle() {
        for (enabled, expected) in [
            (true, b"\0CRT\0\0QUCMD\x1f\x11\0\x11\0\x11".as_slice()),
            (false, b"\0CRT\0\0QUCMD\x1f\x11\0\xff\0\x11".as_slice()),
        ] {
            let report = command_report(&vibro_feedback_command(enabled));
            assert_eq!(&report[..17], expected);
            assert!(report[17..].iter().all(|byte| *byte == 0));
            assert_eq!(report.len(), REPORT_SIZE);
        }
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
