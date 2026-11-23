use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::GlobalHotKeyManager;

pub struct HotkeyIds {
    pub type_mode: u32,
    pub kill_switch: u32,
    pub ptt: u32,
}

pub fn register_hotkeys() -> Result<(GlobalHotKeyManager, HotkeyIds), global_hotkey::Error> {
    let manager = GlobalHotKeyManager::new()?;

    let type_mode = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyM);
    manager.register(type_mode)?;

    let kill_switch = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Escape);
    manager.register(kill_switch)?;

    let ptt = HotKey::new(Some(Modifiers::CONTROL), Code::Space);
    manager.register(ptt)?;

    Ok((
        manager,
        HotkeyIds {
            type_mode: type_mode.id(),
            kill_switch: kill_switch.id(),
            ptt: ptt.id(),
        },
    ))
}
