# Rust driver for Mirabox devices

Async Rust support for communicating with a MiraBox control surface over HID.
Currently only Mirabox N4 Pro is supported.

The crate can connect to the first detected device, upload encoded images to its button displays, control brightness and knob LEDs, and decode button, knob, and touchscreen reports. It requires a supported MiraBox N4 Pro connected to the host. HID access may require operating-system-specific device permissions.

## Example

```rust,no_run
use mirabox_n4pro::{MiraboxN4Pro, PHYSICAL_BUTTON_IMAGE_FORMAT};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(device) = MiraboxN4Pro::connect().await? else {
        println!("No MiraBox N4 Pro found");
        return Ok(());
    };

    println!("Connected to {}", device.serial_number());
    println!("Physical button image size: {:?}", PHYSICAL_BUTTON_IMAGE_FORMAT.size);
    device.set_screen_brightness(50).await?;

    let (_report, event) = device.read_report_event().await?;
    println!("Event: {event:?}");
    Ok(())
}
```

