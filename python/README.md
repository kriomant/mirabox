# MiraBox N4 Pro for Python

Async Python bindings for the Rust `mirabox` crate. Requires a supported HID device and Python 3.9 or newer.

From the `python` directory, install with `python -m pip install .` (requires a Rust toolchain; pip installs maturin as the build backend).

For editable development, from the repository root (with [just](https://just.systems/) and [uv](https://docs.astral.sh/uv/) installed):

```sh
just install-maturin  # Set up maturin in python/.venv (optional separate step)
just install-python   # Build and install the module; also sets up maturin
python/.venv/bin/python -c 'import mirabox; print(mirabox.PHYSICAL_BUTTON_IMAGE_SIZE)'
```

The project-local `.venv` is ignored by Git. Rerun `just install-python` after changing Rust code.

```python
import asyncio
from mirabox import MiraboxN4Pro, PHYSICAL_BUTTON_IMAGE_SIZE

async def main():
    device = await MiraboxN4Pro.connect()
    if device is None:
        print("No MiraBox N4 Pro found")
        return

    print(device.serial_number, PHYSICAL_BUTTON_IMAGE_SIZE)
    # Supply an already encoded JPEG or PNG at 112x112, rotated 180°.
    with open("button.jpg", "rb") as image:
        await device.set_image(MiraboxN4Pro.physical_button_image_key(0), image.read())
    await device.flush()

    while True:
        report, event = await device.read_report_event()
        if event is not None:
            print(event)

asyncio.run(main())
```

All I/O methods return awaitables. `set_image` queues bytes until `flush`; `set_background_image` uploads immediately. `read_event` returns a dictionary or `None` for an unknown report; `read_report_event` returns `(bytes, event)` and retains all 512 raw bytes. Event dictionaries contain `type` (`button`, `knob_pressed`, `knob_rotated`, `touch_start`, `touch_move`, `touch_end`, or `swipe`) plus the corresponding fields (`button`/`pressed`, `knob`/`direction`, `x`/`y`, `code`, or `direction`). Indices for physical buttons and knobs are zero-based. Keep-alive is caller-controlled via `await device.keep_alive()`; the binding does not create a background task.

Other methods: `set_screen_brightness` (0–100), `set_light_brightness` (0–60), `set_led_colors` (four RGB triples), `set_vibro_feedback`, and `clear_background_image`. Image sizes are exposed as `PHYSICAL_BUTTON_IMAGE_SIZE` and `VIRTUAL_BUTTON_IMAGE_SIZE`. Device/transport failures raise `OSError`; invalid brightness/background dimensions raise `ValueError`.
