
use mirajazz::types::DeviceInput;

/// A lossless transport representation for MiraBox's two-byte input reports.
///
/// `DeviceInput` has no generic raw-report variant. `EncoderTwist` is used only
/// as a carrier: its two signed bytes store `control` and `state` respectively.
/// Casting them back to `u8` restores the original bytes exactly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawDeviceInput {
    pub control: u8,
    pub state: u8,
}

impl RawDeviceInput {
    pub const fn new(control: u8, state: u8) -> Self {
        Self { control, state }
    }

    pub fn into_device_input(self) -> DeviceInput {
        DeviceInput::EncoderTwist(vec![self.control as i8, self.state as i8])
    }
}

impl TryFrom<DeviceInput> for RawDeviceInput {
    type Error = DeviceInput;

    fn try_from(input: DeviceInput) -> Result<Self, Self::Error> {
        match input {
            DeviceInput::EncoderTwist(values) if values.len() == 2 => {
                Ok(Self::new(values[0] as u8, values[1] as u8))
            }
            input => Err(input),
        }
    }
}

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


/// A semantically decoded MiraBox N4 Pro input event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceEvent {
    Button {
        button: u8,
        pressed: bool,
    },
    KnobPressed {
        knob: u8,
    },
    KnobRotated {
        knob: u8,
        direction: KnobDirection,
    },
    /// A touch sample reports a touch-bar region index, not a virtual-button index.
    ///
    /// Touch-bar regions do not align regularly with virtual buttons, so this value
    /// must not be used to identify the touched virtual button.
    Touch {
        touchbar_region: u8,
    },
    /// Touch release reports zero-based virtual-button index 0 through 3.
    TouchReleased {
        virtual_button: u8,
    },
    Swipe {
        direction: SwipeDirection,
    },
}

/// A decoded event enriched with its application-level meaning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnrichedDeviceEvent {
    Button {
        button: u8,
        pressed: bool,
    },
    KnobPressed {
        knob: u8,
    },
    KnobRotated {
        knob: u8,
        direction: KnobDirection,
    },
    /// A press on a virtual button.
    ///
    /// This event is emitted for `DeviceEvent::TouchReleased`, because release
    /// reports—not touch samples—contain the virtual-button index.
    Touch {
        button: u8,
    },
    /// A touch-screen swipe direction without virtual-button position data.
    Swipe {
        direction: SwipeDirection,
    },
}

/// Enriches input reports whose raw representation differs from their meaning.
///
/// Touch samples are ignored because their values identify irregular touch-bar
/// regions rather than virtual buttons. A touch-release report identifies the
/// virtual button and is converted to `EnrichedDeviceEvent::Touch`.
#[derive(Debug, Default)]
pub struct EventParser;

impl EventParser {
    pub fn new() -> Self {
        Self
    }

    pub fn parse(&self, event: DeviceEvent) -> Option<EnrichedDeviceEvent> {
        match event {
            DeviceEvent::Touch { .. } => None,
            DeviceEvent::TouchReleased { virtual_button } => {
                Some(EnrichedDeviceEvent::Touch {
                    button: virtual_button,
                })
            }
            DeviceEvent::Button { button, pressed } => {
                Some(EnrichedDeviceEvent::Button { button, pressed })
            }
            DeviceEvent::KnobPressed { knob } => Some(EnrichedDeviceEvent::KnobPressed { knob }),
            DeviceEvent::KnobRotated { knob, direction } => {
                Some(EnrichedDeviceEvent::KnobRotated { knob, direction })
            }
            DeviceEvent::Swipe { direction } => Some(EnrichedDeviceEvent::Swipe { direction }),
        }
    }
}

impl TryFrom<RawDeviceInput> for DeviceEvent {
    type Error = RawDeviceInput;

    fn try_from(raw: RawDeviceInput) -> Result<Self, Self::Error> {
        let event = match (raw.control, raw.state) {
            (1..=10, 0) => Self::Button {
                button: raw.control - 1,
                pressed: false,
            },
            (1..=10, 1) => Self::Button {
                button: raw.control - 1,
                pressed: true,
            },
            (55, 1) => Self::KnobPressed { knob: 0 },
            (53, 1) => Self::KnobPressed { knob: 1 },
            (51, 1) => Self::KnobPressed { knob: 2 },
            (54, 1) => Self::KnobPressed { knob: 3 },
            (160, 0) => Self::KnobRotated {
                knob: 0,
                direction: KnobDirection::CounterClockwise,
            },
            (161, 0) => Self::KnobRotated {
                knob: 0,
                direction: KnobDirection::Clockwise,
            },
            (80, 0) => Self::KnobRotated {
                knob: 1,
                direction: KnobDirection::CounterClockwise,
            },
            (81, 0) => Self::KnobRotated {
                knob: 1,
                direction: KnobDirection::Clockwise,
            },
            (144, 0) => Self::KnobRotated {
                knob: 2,
                direction: KnobDirection::CounterClockwise,
            },
            (145, 0) => Self::KnobRotated {
                knob: 2,
                direction: KnobDirection::Clockwise,
            },
            (112, 0) => Self::KnobRotated {
                knob: 3,
                direction: KnobDirection::CounterClockwise,
            },
            (113, 0) => Self::KnobRotated {
                knob: 3,
                direction: KnobDirection::Clockwise,
            },
            (0, 0..=3) => Self::Touch {
                touchbar_region: raw.state,
            },
            (64..=67, 0) => Self::TouchReleased {
                virtual_button: raw.control - 64,
            },
            (56, 0) => Self::Swipe {
                direction: SwipeDirection::Right,
            },
            (57, 0) => Self::Swipe {
                direction: SwipeDirection::Left,
            },
            (177, 0) => Self::Swipe {
                direction: SwipeDirection::Up,
            },
            (178, 0) => Self::Swipe {
                direction: SwipeDirection::Down,
            },
            _ => return Err(raw),
        };

        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_input_round_trips_through_device_input() {
        let raw = RawDeviceInput::new(177, 255);
        let decoded = RawDeviceInput::try_from(raw.into_device_input()).unwrap();

        assert_eq!(decoded, raw);
    }

    #[test]
    fn decodes_known_events() {
        assert_eq!(
            DeviceEvent::try_from(RawDeviceInput::new(1, 1)).unwrap(),
            DeviceEvent::Button {
                button: 0,
                pressed: true,
            }
        );
        assert_eq!(
            DeviceEvent::try_from(RawDeviceInput::new(161, 0)).unwrap(),
            DeviceEvent::KnobRotated {
                knob: 0,
                direction: KnobDirection::Clockwise,
            }
        );
        assert_eq!(
            DeviceEvent::try_from(RawDeviceInput::new(66, 0)).unwrap(),
            DeviceEvent::TouchReleased { virtual_button: 2 }
        );
        assert_eq!(
            DeviceEvent::try_from(RawDeviceInput::new(56, 0)).unwrap(),
            DeviceEvent::Swipe {
                direction: SwipeDirection::Right
            }
        );
        assert_eq!(
            DeviceEvent::try_from(RawDeviceInput::new(57, 0)).unwrap(),
            DeviceEvent::Swipe {
                direction: SwipeDirection::Left
            }
        );
    }

    #[test]
    fn ignores_touch_samples_and_preserves_swipe_directions() {
        let parser = EventParser::new();

        assert_eq!(parser.parse(device_event(0, 1)), None);
        assert_eq!(parser.parse(device_event(0, 2)), None);
        assert_eq!(
            parser.parse(device_event(57, 0)),
            Some(EnrichedDeviceEvent::Swipe {
                direction: SwipeDirection::Left,
            })
        );
        assert_eq!(
            parser.parse(device_event(177, 0)),
            Some(EnrichedDeviceEvent::Swipe {
                direction: SwipeDirection::Up,
            })
        );
        assert_eq!(
            DeviceEvent::try_from(RawDeviceInput::new(42, 99)),
            Err(RawDeviceInput::new(42, 99))
        );
    }

    #[test]
    fn converts_touch_release_to_virtual_button_press() {
        let parser = EventParser::new();

        assert_eq!(parser.parse(device_event(0, 1)), None);
        assert_eq!(
            parser.parse(device_event(66, 0)),
            Some(EnrichedDeviceEvent::Touch { button: 2 })
        );
    }

    fn device_event(control: u8, state: u8) -> DeviceEvent {
        DeviceEvent::try_from(RawDeviceInput::new(control, state)).unwrap()
    }
}
