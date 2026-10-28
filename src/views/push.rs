use maud::{Markup, html};

/// Where the "enable notifications on this device" control goes. The Reminders and push spec
/// fills it in.
pub const CONTROL_ID: &str = "push-control";

/// The control that turns on notifications for this device, on the welcome and settings pages.
/// A placeholder until push lands.
#[must_use]
pub fn control() -> Markup {
    html! {
        div.placeholder #(CONTROL_ID) {
            "Turning on notifications for this device is coming soon."
        }
    }
}
