use std::{collections::BTreeMap, error::Error, process::ExitCode, time::Duration};

mod widget;

use mirabox::{EnrichedDeviceEvent, MiraboxN4Pro, PHYSICAL_BUTTON_IMAGE_FORMAT};
use widget::{CounterWidget, Updated, Widget, WidgetEvent};

const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(1);

struct WidgetImage {
    key: u8,
    image: image::DynamicImage,
}

struct WidgetRegistry {
    widgets: BTreeMap<u8, Box<dyn Widget>>,
    update_sender: tokio::sync::mpsc::UnboundedSender<u8>,
}

impl WidgetRegistry {
    fn new(update_sender: tokio::sync::mpsc::UnboundedSender<u8>) -> Self {
        Self {
            widgets: BTreeMap::new(),
            update_sender,
        }
    }

    fn register(&mut self, button: u8, make_widget: impl FnOnce(Updated) -> Box<dyn Widget>) {
        assert!(
            MiraboxN4Pro::physical_button_image_key(button).is_some(),
            "widget binding must use a valid physical button number"
        );

        let update_sender = self.update_sender.clone();
        let updated = Box::new(move || {
            let _ = update_sender.send(button);
        });
        assert!(
            self.widgets.insert(button, make_widget(updated)).is_none(),
            "a widget is already registered for button {button}"
        );
    }

    fn images(&self) -> Vec<WidgetImage> {
        self.widgets
            .iter()
            .map(|(&button, widget)| Self::image_for(button, widget.as_ref()))
            .collect()
    }

    fn handle_event(&mut self, button: u8, event: WidgetEvent) {
        if let Some(widget) = self.widgets.get_mut(&button) {
            widget.handle_event(event);
        }
    }

    fn image(&self, button: u8) -> Option<WidgetImage> {
        self.widgets
            .get(&button)
            .map(|widget| Self::image_for(button, widget.as_ref()))
    }

    fn image_for(button: u8, widget: &dyn Widget) -> WidgetImage {
        let key = MiraboxN4Pro::physical_button_image_key(button)
            .expect("widget binding must use a valid physical button number");
        WidgetImage {
            key,
            image: widget.image(PHYSICAL_BUTTON_IMAGE_FORMAT.size),
        }
    }
}

#[tokio::main]
async fn main() -> Result<ExitCode, Box<dyn Error>> {
    let Some(device) = MiraboxN4Pro::connect().await? else {
        eprintln!("No MiraBox N4 Pro found. Connect it, then run this program again.");
        return Ok(ExitCode::FAILURE);
    };

    println!(
        "Connected to MiraBox N4 Pro: serial {}, firmware {}",
        device.serial_number(),
        device.firmware_version().unwrap_or("unknown").trim_end_matches('\0')
    );

    let (widget_update_sender, mut widget_update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut widgets = WidgetRegistry::new(widget_update_sender);
    widgets.register(0, |updated| Box::new(CounterWidget::new(updated)));

    for image in widgets.images() {
        device
            .set_image(image.key, PHYSICAL_BUTTON_IMAGE_FORMAT, image.image)
            .await?;
    }
    device.flush().await?;

    println!("Running widgets; press Control-C to exit.");

    let mut keep_alive = tokio::time::interval(KEEP_ALIVE_INTERVAL);
    keep_alive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = keep_alive.tick() => device.keep_alive().await?,
            Some(button) = widget_update_receiver.recv() => {
                if let Some(image) = widgets.image(button) {
                    device
                        .set_image(image.key, PHYSICAL_BUTTON_IMAGE_FORMAT, image.image)
                        .await?;
                    device.flush().await?;
                }
            }
            event = device.read_event() => {
                if let Some(EnrichedDeviceEvent::Button { button, pressed }) = event? {
                    let widget_event = if pressed {
                        WidgetEvent::Pressed
                    } else {
                        WidgetEvent::Released
                    };
                    widgets.handle_event(button, widget_event);
                }
            }
        }
    }
}
