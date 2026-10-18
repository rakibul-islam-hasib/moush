//! Windows virtual-key codes used by the router and the injector.
//! Values are fixed by the Win32 API; they are duplicated here so the core stays OS-free.

pub const VK_CANCEL: u16 = 0x03;
pub const VK_SHIFT: u16 = 0x10;
pub const VK_CONTROL: u16 = 0x11;
pub const VK_MENU: u16 = 0x12;
pub const VK_PAUSE: u16 = 0x13;
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_SNAPSHOT: u16 = 0x2C;
pub const VK_LWIN: u16 = 0x5B;
pub const VK_RWIN: u16 = 0x5C;
pub const VK_SLEEP: u16 = 0x5F;
pub const VK_NUMLOCK: u16 = 0x90;
pub const VK_SCROLL: u16 = 0x91;
pub const VK_LSHIFT: u16 = 0xA0;
pub const VK_RSHIFT: u16 = 0xA1;
pub const VK_LCONTROL: u16 = 0xA2;
pub const VK_RCONTROL: u16 = 0xA3;
pub const VK_LMENU: u16 = 0xA4;
pub const VK_RMENU: u16 = 0xA5;
/// VK_BROWSER_BACK ..= VK_LAUNCH_APP2: browser, volume, media and launch keys.
pub const VK_BROWSER_FIRST: u16 = 0xA6;
pub const VK_LAUNCH_LAST: u16 = 0xB7;

pub fn is_modifier(vk: u16) -> bool {
    matches!(
        vk,
        VK_SHIFT | VK_CONTROL | VK_MENU | VK_LSHIFT | VK_RSHIFT | VK_LCONTROL | VK_RCONTROL | VK_LMENU | VK_RMENU
            | VK_LWIN | VK_RWIN
    )
}

/// Keys that must be injected by virtual-key code rather than scan code: keys without a
/// scan code, keys whose real scan code needs an E1 prefix (Pause), and multimedia keys.
pub fn inject_by_vk(vk: u16, scan: u16) -> bool {
    scan == 0
        || matches!(vk, VK_PAUSE | VK_CANCEL | VK_SNAPSHOT | VK_SLEEP)
        || (VK_BROWSER_FIRST..=VK_LAUNCH_LAST).contains(&vk)
}
