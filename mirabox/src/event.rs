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
    /// A touch sample reports virtual-button index 0 through 3.
    ///
    /// The raw report `(0, 0)` is also emitted at the start of a touch, so the
    /// start of contact cannot be distinguished from virtual-button 0 using a
    /// single report alone.
    Touch {
        virtual_button: u8,
    },
    /// Touch release reports zero-based virtual-button index 0 through 3.
    TouchReleased {
        virtual_button: u8,
    },
    Swipe {
        direction: SwipeDirection,
    },
    Unknown {
        control: u8,
        state: u8,
    },
}

impl From<RawDeviceInput> for DeviceEvent {
    fn from(raw: RawDeviceInput) -> Self {
        match (raw.control, raw.state) {
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
                virtual_button: raw.state,
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
            _ => Self::Unknown {
                control: raw.control,
                state: raw.state,
            },
        }
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
            DeviceEvent::from(RawDeviceInput::new(1, 1)),
            DeviceEvent::Button {
                button: 0,
                pressed: true,
            }
        );
        assert_eq!(
            DeviceEvent::from(RawDeviceInput::new(161, 0)),
            DeviceEvent::KnobRotated {
                knob: 0,
                direction: KnobDirection::Clockwise,
            }
        );
        assert_eq!(
            DeviceEvent::from(RawDeviceInput::new(66, 0)),
            DeviceEvent::TouchReleased { virtual_button: 2 }
        );
        assert_eq!(
            DeviceEvent::from(RawDeviceInput::new(56, 0)),
            DeviceEvent::Swipe {
                direction: SwipeDirection::Right
            }
        );
        assert_eq!(
            DeviceEvent::from(RawDeviceInput::new(57, 0)),
            DeviceEvent::Swipe {
                direction: SwipeDirection::Left
            }
        );
    }

    #[test]
    fn retains_unrecognised_reports() {
        assert_eq!(
            DeviceEvent::from(RawDeviceInput::new(42, 99)),
            DeviceEvent::Unknown {
                control: 42,
                state: 99,
            }
        );
    }
}
