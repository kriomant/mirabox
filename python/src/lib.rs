//! Async Python bindings for the MiraBox N4 Pro.

use std::sync::Arc;

use mirabox_core::{
    DeviceError, DeviceEvent, KnobDirection, MiraboxN4Pro, PHYSICAL_BUTTON_IMAGE_FORMAT,
    SwipeDirection, VIRTUAL_BUTTON_IMAGE_FORMAT,
};
use pyo3::{
    exceptions::{PyOSError, PyValueError},
    prelude::*,
    types::{PyBytes, PyDict},
};
use pyo3_async_runtimes::tokio::future_into_py;

fn device_error(error: DeviceError) -> PyErr {
    match error {
        DeviceError::InvalidBrightness { .. } | DeviceError::InvalidBackgroundSize { .. } => {
            PyValueError::new_err(error.to_string())
        }
        _ => PyOSError::new_err(error.to_string()),
    }
}

fn event_dict<'py>(py: Python<'py>, event: DeviceEvent) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    match event {
        DeviceEvent::Button { button, pressed } => {
            result.set_item("type", "button")?;
            result.set_item("button", button)?;
            result.set_item("pressed", pressed)?;
        }
        DeviceEvent::KnobPressed { knob } => {
            result.set_item("type", "knob_pressed")?;
            result.set_item("knob", knob)?;
        }
        DeviceEvent::KnobRotated { knob, direction } => {
            result.set_item("type", "knob_rotated")?;
            result.set_item("knob", knob)?;
            result.set_item(
                "direction",
                match direction {
                    KnobDirection::Clockwise => "clockwise",
                    KnobDirection::CounterClockwise => "counterclockwise",
                },
            )?;
        }
        DeviceEvent::TouchStart { x } => {
            result.set_item("type", "touch_start")?;
            result.set_item("x", x)?;
        }
        DeviceEvent::TouchMove { x, y } => {
            result.set_item("type", "touch_move")?;
            result.set_item("x", x)?;
            result.set_item("y", y)?;
        }
        DeviceEvent::TouchEnd { code, x } => {
            result.set_item("type", "touch_end")?;
            result.set_item("code", code)?;
            result.set_item("x", x)?;
        }
        DeviceEvent::Swipe { direction } => {
            result.set_item("type", "swipe")?;
            result.set_item(
                "direction",
                match direction {
                    SwipeDirection::Right => "right",
                    SwipeDirection::Left => "left",
                    SwipeDirection::Up => "up",
                    SwipeDirection::Down => "down",
                },
            )?;
        }
    }
    Ok(result)
}

#[pyclass(name = "MiraboxN4Pro")]
struct PyMiraboxN4Pro {
    device: Arc<MiraboxN4Pro>,
}

#[pymethods]
impl PyMiraboxN4Pro {
    /// Connect to the first detected device; return None if none is present.
    #[staticmethod]
    fn connect<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        future_into_py(py, async {
            Ok(MiraboxN4Pro::connect()
                .await
                .map_err(device_error)?
                .map(|device| Self {
                    device: Arc::new(device),
                }))
        })
    }

    #[staticmethod]
    fn physical_button_image_key(button: u8) -> Option<u8> {
        MiraboxN4Pro::physical_button_image_key(button)
    }

    #[getter]
    fn serial_number(&self) -> String {
        self.device.serial_number().to_owned()
    }

    #[getter]
    fn firmware_version(&self) -> Option<String> {
        self.device.firmware_version().map(str::to_owned)
    }

    /// Queue encoded JPEG/PNG bytes. Call flush to display queued images.
    fn set_image<'py>(&self, py: Python<'py>, key: u8, image: Vec<u8>) -> PyResult<Bound<'py, PyAny>> {
        let device = Arc::clone(&self.device);
        future_into_py(py, async move {
            device.set_image(key, &image).await.map_err(device_error)
        })
    }

    fn flush<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
            let device = Arc::clone(&self.device);
            future_into_py(py, async move {
                device.flush().await.map_err(device_error)
            })
        }

    /// Upload encoded image bytes immediately to the specified rectangle.
    fn set_background_image<'py>(
        &self,
        py: Python<'py>,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        selector: [u8; 2],
        image: Vec<u8>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let device = Arc::clone(&self.device);
        future_into_py(py, async move {
            device
                .set_background_image(x, y, width, height, selector, &image)
                .await
                .map_err(device_error)
        })
    }

    fn clear_background_image<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = Arc::clone(&self.device);
        future_into_py(py, async move {
            device.clear_background_image().await.map_err(device_error)
        })
    }

    fn set_screen_brightness<'py>(&self, py: Python<'py>, brightness: u8) -> PyResult<Bound<'py, PyAny>> {
        let device = Arc::clone(&self.device);
        future_into_py(py, async move {
            device
                .set_screen_brightness(brightness)
                .await
                .map_err(device_error)
        })
    }

    fn set_light_brightness<'py>(&self, py: Python<'py>, brightness: u8) -> PyResult<Bound<'py, PyAny>> {
        let device = Arc::clone(&self.device);
        future_into_py(py, async move {
            device
                .set_light_brightness(brightness)
                .await
                .map_err(device_error)
        })
    }

    /// Colors are four RGB triples, one per knob.
    fn set_led_colors<'py>(&self, py: Python<'py>, colors: [[u8; 3]; 4]) -> PyResult<Bound<'py, PyAny>> {
        let device = Arc::clone(&self.device);
        future_into_py(py, async move {
            device.set_led_colors(colors).await.map_err(device_error)
        })
    }

    fn set_vibro_feedback<'py>(&self, py: Python<'py>, enabled: bool) -> PyResult<Bound<'py, PyAny>> {
        let device = Arc::clone(&self.device);
        future_into_py(py, async move {
            device
                .set_vibro_feedback(enabled)
                .await
                .map_err(device_error)
        })
    }

    fn keep_alive<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = Arc::clone(&self.device);
        future_into_py(py, async move { device.keep_alive().await.map_err(device_error) })
    }

    /// Wait for a report; unknown reports return None.
    fn read_event<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = Arc::clone(&self.device);
        future_into_py(py, async move {
            let event = device.read_event().await.map_err(device_error)?;
            Python::attach(|py| event.map(|event| event_dict(py, event).map(Bound::unbind)).transpose())
        })
    }

    /// Return all 512 report bytes and the decoded event, if recognized.
    fn read_report_event<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = Arc::clone(&self.device);
        future_into_py(py, async move {
            let (report, event) = device.read_report_event().await.map_err(device_error)?;
            Python::attach(|py| {
                Ok((
                    PyBytes::new(py, &report).unbind(),
                    event.map(|event| event_dict(py, event).map(Bound::unbind)).transpose()?,
                ))
            })
        })
    }
}

#[pymodule]
fn mirabox(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyMiraboxN4Pro>()?;
    m.add("PHYSICAL_BUTTON_IMAGE_SIZE", PHYSICAL_BUTTON_IMAGE_FORMAT.size)?;
    m.add("VIRTUAL_BUTTON_IMAGE_SIZE", VIRTUAL_BUTTON_IMAGE_FORMAT.size)?;
    Ok(())
}
