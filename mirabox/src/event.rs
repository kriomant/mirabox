#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnobDirection {
    CounterClockwise,
    Clockwise,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SwipeDirection {
    Right,
    Left,
    Up,
    Down,
}

/// A decoded MiraBox N4 Pro vendor input report.
///
/// Touch ending codes are not universal button indexes: map them using the
/// current host-side layout and coordinates. Keep raw bytes for unknown reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceEvent {
    Button { button: u8, pressed: bool },
    KnobPressed { knob: u8 },
    KnobRotated { knob: u8, direction: KnobDirection },
    TouchStart { x: u16 },
    TouchMove { x: u16, y: u16 },
    TouchEnd { code: u8, x: u16 },
    Swipe { direction: SwipeDirection },
}

impl DeviceEvent {
    /// Decodes the captured `ACK/OK` and `ACK/ARX` families directly from a
    /// vendor input report. Returns `None` for incomplete or unknown reports.
    pub fn decode(report: &[u8]) -> Option<Self> {
        if report.starts_with(b"ACK\0ARX\0\0") {
            if report.len() < 14 {
                return None;
            }
            return Some(Self::TouchMove {
                x: u16::from_be_bytes([report[10], report[11]]),
                y: u16::from_be_bytes([report[12], report[13]]),
            });
        }
        if !report.starts_with(b"ACK\0\0OK\0\0") || report.len() < 11 {
            return None;
        }

        let (code, state) = (report[9], report[10]);
        let x = || (report.len() >= 13).then(|| u16::from_be_bytes([report[11], report[12]]));
        match (code, state) {
            (1..=10, 0 | 1) => Some(Self::Button {
                button: code - 1,
                pressed: state == 1,
            }),
            (0, 1) => Some(Self::TouchStart { x: x()? }),
            (0 | 64..=67, 0) => Some(Self::TouchEnd { code, x: x()? }),
            (55, 1) => Some(Self::KnobPressed { knob: 0 }),
            (53, 1) => Some(Self::KnobPressed { knob: 1 }),
            (51, 1) => Some(Self::KnobPressed { knob: 2 }),
            (54, 1) => Some(Self::KnobPressed { knob: 3 }),
            (160, 0) => Some(Self::KnobRotated {
                knob: 0,
                direction: KnobDirection::CounterClockwise,
            }),
            (161, 0) => Some(Self::KnobRotated {
                knob: 0,
                direction: KnobDirection::Clockwise,
            }),
            (80, 0) => Some(Self::KnobRotated {
                knob: 1,
                direction: KnobDirection::CounterClockwise,
            }),
            (81, 0) => Some(Self::KnobRotated {
                knob: 1,
                direction: KnobDirection::Clockwise,
            }),
            (144, 0) => Some(Self::KnobRotated {
                knob: 2,
                direction: KnobDirection::CounterClockwise,
            }),
            (145, 0) => Some(Self::KnobRotated {
                knob: 2,
                direction: KnobDirection::Clockwise,
            }),
            (112, 0) => Some(Self::KnobRotated {
                knob: 3,
                direction: KnobDirection::CounterClockwise,
            }),
            (113, 0) => Some(Self::KnobRotated {
                knob: 3,
                direction: KnobDirection::Clockwise,
            }),
            (56, 0) => Some(Self::Swipe {
                direction: SwipeDirection::Right,
            }),
            (57, 0) => Some(Self::Swipe {
                direction: SwipeDirection::Left,
            }),
            (177, 0) => Some(Self::Swipe {
                direction: SwipeDirection::Up,
            }),
            (178, 0) => Some(Self::Swipe {
                direction: SwipeDirection::Down,
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_report(code: u8, state: u8, x: u16) -> [u8; 512] {
        let mut report = [0; 512];
        report[..9].copy_from_slice(b"ACK\0\0OK\0\0");
        report[9] = code;
        report[10] = state;
        report[11..13].copy_from_slice(&x.to_be_bytes());
        report
    }

    #[test]
    fn decodes_buttons_knobs_and_swipes() {
        assert_eq!(
            DeviceEvent::decode(&ok_report(1, 1, 0)),
            Some(DeviceEvent::Button {
                button: 0,
                pressed: true
            })
        );
        assert_eq!(
            DeviceEvent::decode(&ok_report(10, 0, 0)),
            Some(DeviceEvent::Button {
                button: 9,
                pressed: false
            })
        );
        assert_eq!(
            DeviceEvent::decode(&ok_report(0xa1, 0, 0)),
            Some(DeviceEvent::KnobRotated {
                knob: 0,
                direction: KnobDirection::Clockwise,
            })
        );
        assert_eq!(
            DeviceEvent::decode(&ok_report(0x35, 1, 0)),
            Some(DeviceEvent::KnobPressed { knob: 1 })
        );
        for (code, direction) in [
            (0x38, SwipeDirection::Right),
            (0x39, SwipeDirection::Left),
            (0xb1, SwipeDirection::Up),
            (0xb2, SwipeDirection::Down),
        ] {
            assert_eq!(
                DeviceEvent::decode(&ok_report(code, 0, 0)),
                Some(DeviceEvent::Swipe { direction })
            );
        }
    }

    #[test]
    fn decodes_touch_fields_at_their_respective_offsets() {
        assert_eq!(
            DeviceEvent::decode(&ok_report(0, 1, 291)),
            Some(DeviceEvent::TouchStart { x: 291 })
        );
        let mut movement = [0; 512];
        movement[..9].copy_from_slice(b"ACK\0ARX\0\0");
        movement[10..14].copy_from_slice(&[0x01, 0x23, 0x00, 0x78]);
        assert_eq!(
            DeviceEvent::decode(&movement),
            Some(DeviceEvent::TouchMove { x: 291, y: 120 })
        );
        for code in [0, 0x40, 0x41, 0x42, 0x43] {
            assert_eq!(
                DeviceEvent::decode(&ok_report(code, 0, 300)),
                Some(DeviceEvent::TouchEnd { code, x: 300 })
            );
        }
    }

    #[test]
    fn rejects_unknown_families_codes_and_incomplete_reports() {
        let movement = b"ACK\0ARX\0\0\0\x01\x23\0\x78";
        assert_eq!(DeviceEvent::decode(&movement[..13]), None);
        assert_eq!(DeviceEvent::decode(&ok_report(0x42, 99, 0)), None);
        assert_eq!(DeviceEvent::decode(&ok_report(0, 2, 0)), None);
        assert_eq!(DeviceEvent::decode(&ok_report(0, 1, 0)[..12]), None);
        assert_eq!(DeviceEvent::decode(b"ACK\0\0OK\0\0\x01"), None);
        assert_eq!(DeviceEvent::decode(b"BAD\0\0OK\0\0\x01\x01"), None);
    }
}
