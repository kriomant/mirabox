# MiraBox N4 Pro vendor input reports

The vendor collection returns 512-byte reports. `DeviceEvent::decode(&report)`
decodes the captured `ACK/OK` and `ACK/ARX` families directly from report bytes;
it returns `None` for unknown or incomplete reports. Use
`MiraboxN4Pro::read_report_event()` to receive both the full report and its
optional decoded event. `read_event()` returns just the optional event.

## Physical buttons

In `ACK/OK` reports, byte 9 is the one-based physical button ID (1–10), and
byte 10 is `1` for press or `0` for release. `DeviceEvent::Button` uses a
zero-based button index. Overlapping square-button presses are not independent
rollover keys: the first-held key can appear released and pressed again.

## Rotary dials

The following `ACK/OK` byte-9 codes are decoded for the four dials from left
to right. Rotations have byte 10 = `0`; mechanical pushes have byte 10 = `1`.

| Dial index | Clockwise | Counterclockwise | Push |
| --- | --- | --- | --- |
| 0 | `a1` | `a0` | `37` |
| 1 | `51` | `50`* | `35` |
| 2 | `91` | `90`* | `33` |
| 3 | `71` | `70`* | `36` |

*These counterclockwise codes are currently inferred in the implementation,
not confirmed by the capture-backed protocol spec.

## Touchscreen

- `ACK/OK` code `00`, state `01`: `TouchStart { x }`, with BE16 X at bytes 11–12.
- `ACK/ARX`: `TouchMove { x, y }`, with BE16 X at bytes 10–11 and BE16 Y at bytes 12–13. It is not a second code/state report.
- `ACK/OK` code `00` or `40`–`43`, state `00`: `TouchEnd { code, x }`, with BE16 X at bytes 11–12. The code is **not** a universal virtual-button index; map it using the current layout and touch position.
- `ACK/OK` codes `38`, `39`, `b1`, `b2`, state `00`: `Swipe { Right, Left, Up, Down }`, respectively.

The device reports one touch stream, not independent multitouch slots. A
square button held before a new touch can suppress that touch; the reverse
order does not necessarily suppress the square-button events.

## Raw transport

`read_raw_report::<N>()` returns the first `N` bytes as a fixed-size array
without a heap allocation. Choose `N = 14` to cover known touch fields or
`N = 512` to retain every byte, including unknown firmware data. If the
transport read is shorter than `N`, it returns an error. `N > 512` is invalid.
