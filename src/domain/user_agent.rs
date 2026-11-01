//! Telling a device and browser from a user agent, to name what a Member set up there.

/// The device, the browser, or both when they can be told, such as "iPhone Safari".
#[must_use]
pub fn describe(user_agent: &str) -> Option<String> {
    match (device(user_agent), browser(user_agent)) {
        (Some(device), Some(browser)) => Some(format!("{device} {browser}")),
        (Some(part), None) | (None, Some(part)) => Some(part.to_owned()),
        (None, None) => None,
    }
}

/// The first device whose marker the user agent contains. iPhone and iPad come before Mac, and
/// Android and Chrome OS before Linux, because their user agents name both.
fn device(user_agent: &str) -> Option<&'static str> {
    [
        ("iPhone", "iPhone"),
        ("iPad", "iPad"),
        ("Android", "Android"),
        ("CrOS", "Chromebook"),
        ("Macintosh", "Mac"),
        ("Windows", "Windows"),
        ("Linux", "Linux"),
    ]
    .into_iter()
    .find_map(|(marker, device)| user_agent.contains(marker).then_some(device))
}

/// The first browser whose marker the user agent contains. Most browsers also claim to be
/// Chrome or Safari, so those come last, Chrome before Safari.
fn browser(user_agent: &str) -> Option<&'static str> {
    [
        ("Edg", "Edge"),
        ("OPR/", "Opera"),
        ("SamsungBrowser", "Samsung Internet"),
        ("Firefox/", "Firefox"),
        ("FxiOS", "Firefox"),
        ("CriOS", "Chrome"),
        ("Chrome/", "Chrome"),
        ("Safari/", "Safari"),
    ]
    .into_iter()
    .find_map(|(marker, browser)| user_agent.contains(marker).then_some(browser))
}
