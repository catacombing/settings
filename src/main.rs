use std::cell::RefCell;
use std::rc::Rc;

use glib::{clone, ExitCode, IsA};
use gtk4::prelude::*;
use gtk4::{
    ActionBar, Application, ApplicationWindow, Button, Label, Orientation, ScrolledWindow, Stack,
    StackTransitionType, Widget,
};

use crate::wifi::WiFi;

mod wifi;

/// Wayland application ID.
const APP_ID: &str = "catacomb.Settings";

/// Name of the settings overview panel.
const ROOT_NAME: &str = "index";

#[tokio::main]
async fn main() -> ExitCode {
    // Setup application.
    let application = Application::builder().application_id(APP_ID).build();

    // Handle application activation event.
    application.connect_activate(activate);

    // Run application.
    application.run()
}

/// Bootstrap UI.
fn activate(app: &Application) {
    // Configure window settings.
    let window = ApplicationWindow::builder().application(app).title("Settings").build();

    // Create root panel for settings overview.
    let index_box = gtk4::Box::new(Orientation::Vertical, 0);
    let index = ScrolledWindow::new();
    index.set_child(Some(&index_box));

    // Create navigator, allowing navigation between all panels.
    let navigator = Navigator::new();
    window.set_child(Some(&navigator.stack));

    // Add root widget showing all available options.
    navigator.add(&index, ROOT_NAME);

    // Add all available settings pages.
    let panels = vec![WiFi::new(navigator.clone())];

    // Add all panels recursively.
    for panel in &panels {
        // Add overview button to switch to this panel.
        let title = panel.title().to_owned();
        let button = Button::with_label(&title);
        button.connect_clicked(clone!(@strong navigator => move |_| navigator.show(&title)));
        index_box.append(&button);

        // Wrap panel to add a footer.
        let title = panel.title();
        let footered = Footered::new(navigator.clone(), &panel.widget(), title);

        // Add settings' buttons to the start of the footer bar.
        for button in panel.footer_buttons() {
            footered.footer.pack_start(button);
        }

        // Make panel available to the stack.
        navigator.add(&footered.panel_box, title);
    }

    // Show window.
    window.present();
}

/// Single settings page.
pub trait SettingsPanel {
    /// Settings title.
    fn title(&self) -> &str;

    /// Root widget element.
    fn widget(&self) -> Widget;

    /// Additional footer buttons.
    fn footer_buttons(&self) -> &[Widget] {
        &[]
    }
}

/// Navigator allowing transition between different [`SettingsPanel`]
/// implementations.
#[derive(Clone, Default)]
pub struct Navigator {
    nodes: Rc<RefCell<Vec<NavigatorNode>>>,
    stack: Stack,
}

impl Navigator {
    fn new() -> Self {
        Self::default()
    }

    /// Pop the current panel, returning to its parent.
    pub fn pop(&self) {
        let mut nodes = self.nodes.borrow_mut();

        // Update the visible element.
        let parent = nodes.len().checked_sub(2).and_then(|index| nodes.get(index));
        match parent {
            Some(NavigatorNode { name, .. }) => {
                self.stack.set_visible_child_full(name, StackTransitionType::SlideRight)
            },
            None => self.stack.set_visible_child_full(ROOT_NAME, StackTransitionType::SlideRight),
        }

        // Destroy node if it was a temporary child.
        if let Some(NavigatorNode { name, destroy_on_pop: true }) = nodes.pop() {
            if let Some(child) = self.stack.child_by_name(&name) {
                self.stack.remove(&child);
            }
        }
    }

    /// Show a different panel, adding it to the top of the stack.
    pub fn show(&self, name: &str) {
        let mut nodes = self.nodes.borrow_mut();
        nodes.push(NavigatorNode::new(name, false));
        self.stack.set_visible_child_full(name, StackTransitionType::SlideLeft);
    }

    /// Add an element to the underlying stack.
    pub fn add(&self, widget: &impl IsA<Widget>, name: &str) {
        self.stack.add_named(widget, Some(name));
    }

    /// Add a temporary child element, automatically removing it after it is
    /// popped.
    pub fn show_child(&self, navigator: Navigator, widget: &impl IsA<Widget>, name: &str) {
        // Add child to stack.
        let footered = Footered::new(navigator, widget, name);
        self.add(&footered.panel_box, name);

        // Add it to the active stack, requesting destruction on pop.
        let mut nodes = self.nodes.borrow_mut();
        nodes.push(NavigatorNode::new(name, true));

        // Make it visible.
        self.stack.set_visible_child_full(name, StackTransitionType::SlideLeft);
    }
}

/// Node in the navigator chain.
#[derive(Default)]
struct NavigatorNode {
    name: String,
    destroy_on_pop: bool,
}

impl NavigatorNode {
    fn new(name: &str, destroy_on_pop: bool) -> Self {
        Self { destroy_on_pop, name: name.into() }
    }
}

/// Wrap widget into a panel with navigation footer.
struct Footered {
    panel_box: gtk4::Box,
    footer: ActionBar,
}

impl Footered {
    fn new(navigator: Navigator, widget: &impl IsA<Widget>, name: &str) -> Self {
        // Create title for this page.
        let title_label = Label::new(Some(name));

        // Create button to go back to the root overview.
        let back_button = Button::with_label("←");
        back_button.connect_clicked(move |_| navigator.pop());

        // Create footer with title and back button.
        let footer = ActionBar::new();
        footer.set_center_widget(Some(&title_label));
        footer.pack_end(&back_button);

        // Ensure widget takes free space instead of footer.
        widget.set_vexpand(true);

        // Append footer to the bottom of the widget.
        let panel_box = gtk4::Box::new(Orientation::Vertical, 0);
        panel_box.append(widget);
        panel_box.append(&footer);

        Self { footer, panel_box }
    }
}
