use image::{DynamicImage, ImageBuffer, Rgb};


const DIGIT_GLYPHS: [[&str; 5]; 10] = [
    ["111", "1 1", "1 1", "1 1", "111"],
    [" 1 ", "11 ", " 1 ", " 1 ", "111"],
    ["111", "  1", "111", "1  ", "111"],
    ["111", "  1", "111", "  1", "111"],
    ["1 1", "1 1", "111", "  1", "  1"],
    ["111", "1  ", "111", "  1", "111"],
    ["111", "1  ", "111", "1 1", "111"],
    ["111", "  1", "  1", "  1", "  1"],
    ["111", "1 1", "111", "1 1", "111"],
    ["111", "1 1", "111", "  1", "111"],
];

/// A physical-button input delivered to a [`Widget`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WidgetEvent {
    Pressed,
    Released,
}

/// Notifies the owner that a widget's image should be redrawn.
pub type Updated = Box<dyn Fn() + Send>;

/// A stateful control displayed on a physical MiraBox button.
pub trait Widget: Send {
    /// Returns the current image for the button with the supplied dimensions.
    fn image(&self, size: (usize, usize)) -> DynamicImage;

    /// Handles an input for this widget's button.
    fn handle_event(&mut self, event: WidgetEvent);
}

/// A sample widget that counts presses of one physical button.
pub struct CounterWidget {
    count: u64,
    updated: Updated,
}

impl CounterWidget {
    pub fn new(updated: Updated) -> Self {
        Self { count: 0, updated }
    }

    #[cfg(test)]
    fn count(&self) -> u64 {
        self.count
    }
}

impl Widget for CounterWidget {
    fn image(&self, (width, height): (usize, usize)) -> DynamicImage {
        let (width, height) = (width as u32, height as u32);
        let mut image = ImageBuffer::from_pixel(width, height, Rgb([32, 64, 160]));
        draw_number(&mut image, self.count, width, height);
        DynamicImage::ImageRgb8(image)
    }

    fn handle_event(&mut self, event: WidgetEvent) {
        if event == WidgetEvent::Pressed {
            self.count = self.count.saturating_add(1);
            (self.updated)();
        }
    }
}

fn draw_number(image: &mut ImageBuffer<Rgb<u8>, Vec<u8>>, number: u64, width: u32, height: u32) {
    let digits = number.to_string();
    let digit_count = digits.len() as u32;
    let scale = (width / (digit_count * 4)).min(height / 6).max(1);
    let glyph_width = 3 * scale;
    let glyph_height = 5 * scale;
    let spacing = scale;
    let number_width = digit_count * glyph_width + (digit_count - 1) * spacing;
    let origin_x = (width - number_width) / 2;
    let origin_y = (height - glyph_height) / 2;

    for (digit_index, digit) in digits.bytes().enumerate() {
        let digit_x = origin_x + digit_index as u32 * (glyph_width + spacing);
        for (row, pattern) in DIGIT_GLYPHS[(digit - b'0') as usize].iter().enumerate() {
            for (column, pixel) in pattern.bytes().enumerate() {
                if pixel == b'1' {
                    for y in 0..scale {
                        for x in 0..scale {
                            image.put_pixel(
                                digit_x + column as u32 * scale + x,
                                origin_y + row as u32 * scale + y,
                                Rgb([255, 255, 255]),
                            );
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_starts_at_zero_and_notifies_on_press() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let update_count = Arc::new(AtomicUsize::new(0));
        let update_count_for_callback = Arc::clone(&update_count);
        let mut widget = CounterWidget::new(Box::new(move || {
            update_count_for_callback.fetch_add(1, Ordering::Relaxed);
        }));

        assert_eq!(widget.count(), 0);
        widget.handle_event(WidgetEvent::Released);
        assert_eq!(update_count.load(Ordering::Relaxed), 0);

        widget.handle_event(WidgetEvent::Pressed);
        assert_eq!(widget.count(), 1);
        assert_eq!(update_count.load(Ordering::Relaxed), 1);
    }
}
