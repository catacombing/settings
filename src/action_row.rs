//! ActionRow widget.
//!
//! This is a reimplementation of libadwaita's `ActionRow`, without having to
//! rely on libadwaita.

use gtk4::prelude::*;
use gtk4::{
    Align, EventSequenceState, GestureClick, IconSize, Image, Label, ListBoxRow, Orientation,
};

/// Action row widget.
///
/// This widget creates a `ListBoxRow` designed to be displayed in a
/// [`gtk4::ListBox`].
#[derive(Default)]
pub struct ActionRowBuilder<'a> {
    label: &'a str,
    description: Option<&'a str>,
    start_icon: Option<Image>,
    end_icon: Option<Image>,
    handler: Option<Box<dyn Fn()>>,
}

impl<'a> ActionRowBuilder<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            description: Default::default(),
            start_icon: Default::default(),
            end_icon: Default::default(),
            handler: Default::default(),
        }
    }

    /// Add a widget subtext.
    pub fn with_description(&mut self, description: Option<&'a str>) -> &mut Self {
        self.description = description;
        self
    }

    /// Add an icon to the start of the row.
    pub fn with_start_icon(&mut self, icon: Image) -> &mut Self {
        self.start_icon = Some(icon);
        self
    }

    /// Add an icon to the end of the row.
    pub fn with_end_icon(&mut self, icon: Image) -> &mut Self {
        self.end_icon = Some(icon);
        self
    }

    /// Add click/touch handler.
    pub fn with_connect_click<F: Fn() + 'static>(&mut self, handler: F) -> &mut Self {
        self.handler = Some(Box::new(handler));
        self
    }

    /// Build the action row.
    pub fn build(&mut self) -> ListBoxRow {
        // Create vertical box for the label and description.
        let text_box = gtk4::Box::new(Orientation::Vertical, 0);
        text_box.set_valign(Align::Center);
        text_box.set_halign(Align::Start);
        text_box.set_hexpand(true);
        text_box.set_margin_start(10);
        text_box.add_css_class("actionrow-text");

        // Add main action text.
        let label = Label::new(Some(self.label));
        label.set_halign(Align::Start);
        text_box.append(&label);

        // Add subtext below the main label.
        if let Some(description) = self.description {
            let description = Label::new(Some(description));
            description.set_halign(Align::Start);
            text_box.append(&description);
        }

        // Create horizontal box to hold all widgets.
        let center_box = gtk4::Box::new(Orientation::Horizontal, 0);

        // Add optional icon at the start.
        if let Some(start_icon) = &self.start_icon {
            start_icon.set_icon_size(IconSize::Large);
            start_icon.set_margin_start(10);
            start_icon.set_margin_end(10);
            center_box.append(start_icon);
        }

        // Add action row labels.
        center_box.append(&text_box);
        center_box.set_size_request(-1, 50);

        // Add optional icon at the end.
        if let Some(end_icon) = &self.end_icon {
            end_icon.set_margin_start(10);
            end_icon.set_margin_end(10);
            center_box.append(end_icon);
        }

        // Add touch/click handler.
        if let Some(handler) = self.handler.take() {
            let gesture = GestureClick::new();
            gesture.connect_released(move |gesture, _, _, _| {
                gesture.set_state(EventSequenceState::Claimed);
                handler();
            });
            center_box.add_controller(gesture);
        }

        // Create row for the `ListBox`.
        let list_row = ListBoxRow::new();
        list_row.set_child(Some(&center_box));
        list_row.set_activatable(false);

        list_row
    }
}
