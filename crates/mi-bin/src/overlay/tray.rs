use image::{Rgba, RgbaImage};
use muda::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::TrayIconBuilder;

pub const MENU_TALK_ID: &str = "mi_talk";
pub const MENU_QUIT_ID: &str = "mi_quit";

fn create_mi_icon() -> tray_icon::Icon {
    let size = 32u32;
    let mut img = RgbaImage::new(size, size);
    let center = (size / 2) as f64;
    let radius = (size / 2 - 2) as f64;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f64 - center;
            let dy = y as f64 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= radius {
                let alpha = if dist > radius - 1.5 {
                    ((radius - dist) / 1.5 * 255.0) as u8
                } else {
                    255
                };
                img.put_pixel(x, y, Rgba([100, 149, 237, alpha]));
            }
        }
    }

    let (w, h) = (img.width(), img.height());
    tray_icon::Icon::from_rgba(img.into_raw(), w, h).expect("failed to create tray icon")
}

pub fn create_tray() -> tray_icon::TrayIcon {
    let menu = Menu::new();

    let talk_item = MenuItem::with_id(MENU_TALK_ID, "Talk to mi", true, None::<muda::accelerator::Accelerator>);
    let quit_item = MenuItem::with_id(MENU_QUIT_ID, "Quit", true, None::<muda::accelerator::Accelerator>);

    menu.append(&talk_item).unwrap();
    menu.append(&PredefinedMenuItem::separator()).unwrap();
    menu.append(&quit_item).unwrap();

    TrayIconBuilder::new()
        .with_icon(create_mi_icon())
        .with_tooltip("mi — assistant")
        .with_menu(Box::new(menu))
        .build()
        .expect("failed to create tray icon")
}
