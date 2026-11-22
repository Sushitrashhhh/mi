use notify_rust::Notification;

pub fn show_notification(title: &str, body: &str) {
    let _ = Notification::new()
        .appname("mi")
        .summary(title)
        .body(body)
        .timeout(5000)
        .show();
}

pub fn show_error(message: &str) {
    show_notification("mi — error", message);
}

pub fn show_approval_needed(tool: &str, summary: &str) {
    show_notification(
        &format!("mi — approval needed: {}", tool),
        summary,
    );
}
