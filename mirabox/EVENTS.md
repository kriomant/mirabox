# MiraBox N4 Pro input reports

The device emits a raw two-byte report, written here as `(control, state)`.
`state = 1` means pressed and `state = 0` means released for physical buttons.

All indices exposed by `DeviceEvent` are **zero-based**, even when the device's
raw controls are one-based.

## Physical buttons

| Raw report | Decoded event |
| --- | --- |
| `(1..=10, 1)` | `DeviceEvent::Button { button: control - 1, pressed: true }` |
| `(1..=10, 0)` | `DeviceEvent::Button { button: control - 1, pressed: false }` |

The device cannot faithfully report two physical buttons held at once: pressing a
second button reports a release for the first; releasing the second reports a
press for the first.

## Knobs

| Control | Press report | Counter-clockwise | Clockwise |
| --- | --- | --- | --- |
| 0 | `(55, 1)` | `(160, 0)` | `(161, 0)` |
| 1 | `(53, 1)` | `(80, 0)` | `(81, 0)` |
| 2 | `(51, 1)` | `(144, 0)` | `(145, 0)` |
| 3 | `(54, 1)` | `(112, 0)` | `(113, 0)` |

A knob has no release report. While a knob is pressed, input from another knob
is not reported. Knobs continue to report while a physical button or touch is
active.

## Touch screen

| Raw report | Decoded event | Meaning |
| --- | --- | --- |
| `(0, 0..=3)` | `Touch { touchbar_region }` | Periodic touch sample, roughly 20 Hz |
| `(64..=67, 0)` | `TouchReleased { virtual_button: control - 64 }` | Release on virtual buttons 0 through 3 |
| `(56, 0)` | `DeviceEvent::Swipe { Right }` | Swipe right |
| `(57, 0)` | `DeviceEvent::Swipe { Left }` | Swipe left |
| `(177, 0)` | `DeviceEvent::Swipe { Up }` | Swipe up |
| `(178, 0)` | `DeviceEvent::Swipe { Down }` | Swipe down |

`Touch { touchbar_region }` is a device quirk: its value identifies a touch-bar
region, **not** a virtual button. The regions do not map regularly to virtual
buttons, so `EventParser` ignores all touch samples. Swipe reports are preserved
as `EnrichedDeviceEvent::Swipe { direction }`; no virtual-button position is
available for them. In contrast, `TouchReleased { virtual_button }` does contain
the zero-based virtual-button index. `EventParser` converts it into one
`EnrichedDeviceEvent::Touch { button }` press event.

`MiraboxN4Pro::read_event` returns these enriched events; `DeviceEvent` remains
the stateless raw-report decoding API. Unknown raw reports are dropped.

Touch input is suppressed while a physical button is pressed. In the opposite
direction, physical button events continue to report while a touch is active.

## `DeviceInput` transport encoding

`mirajazz::types::DeviceInput` cannot represent arbitrary raw `(control,
state)` pairs. `RawDeviceInput::into_device_input` therefore uses
`DeviceInput::EncoderTwist(vec![control as i8, state as i8])` as an internal,
lossless two-byte carrier. `RawDeviceInput::try_from` reverses it immediately
by casting both values to `u8`, preserving values such as controls 160--178.

This carrier is not a native encoder event and must be decoded before the
resulting `DeviceInput` is used for application behavior. Unknown reports are dropped by `MiraboxN4Pro::read_event`.

## Simultaneous-input behavior

- Knob events are reported normally while a physical button is held.
- Knob and touch events are reported normally while touch is active.
- Button events are reported normally while a knob is held.
- Touch events are reported while a knob is held.
