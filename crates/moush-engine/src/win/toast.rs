//! Windows toast notifications for this unpackaged desktop app, through WinRT
//! (`Windows.UI.Notifications`) with the `windows` crate the app already uses.
//!
//! Toasts need an AppUserModelID that Windows knows. The NSIS installer gives the Start
//! menu shortcut the app identifier as its AUMID; for portable runs (for example straight
//! from `target\`) the same id is also registered under
//! `HKCU\Software\Classes\AppUserModelId\<id>` with a display name and icon, which Windows
//! 10 1903 and later accept for unpackaged apps. Delivery runs on its own thread: callers
//! only queue a message.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::mpsc::{self, SyncSender};
use std::sync::Arc;
use std::thread;

use windows::core::{IInspectable, Ref, HSTRING, PCWSTR};
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager, ToastNotifier};

/// Queued notifications beyond this are dropped (a burst of events is not worth more).
const QUEUE: usize = 16;

/// Shows toasts from a background thread.
pub struct Toaster {
    tx: SyncSender<(String, String)>,
}

impl Toaster {
    /// Starts the delivery thread. `icon` is a PNG shown with the app name when the app
    /// is not installed; `on_click` runs (on a WinRT thread) when the user clicks a toast.
    pub fn start(aumid: &str, display_name: &str, icon: Option<PathBuf>, on_click: impl Fn() + Send + Sync + 'static) -> Self {
        let (tx, rx) = mpsc::sync_channel::<(String, String)>(QUEUE);
        let aumid = aumid.to_owned();
        let display_name = display_name.to_owned();
        let on_click: Arc<dyn Fn() + Send + Sync> = Arc::new(on_click);
        let spawned = thread::Builder::new().name("notifications".into()).spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            register(&aumid, &display_name, icon.as_ref());
            let notifier = match ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(aumid.as_str())) {
                Ok(n) => n,
                Err(e) => {
                    log::warn!("notifications are unavailable: {e}");
                    return;
                }
            };
            for (title, body) in rx {
                if let Err(e) = show(&notifier, &title, &body, &on_click) {
                    log::warn!("could not show a notification: {e}");
                }
            }
        });
        if let Err(e) = spawned {
            log::warn!("cannot start the notification thread: {e}");
        }
        Self { tx }
    }

    /// Queues a notification; never blocks. Returns false if it was dropped.
    pub fn show(&self, title: &str, body: &str) -> bool {
        self.tx.try_send((title.to_owned(), body.to_owned())).is_ok()
    }
}

fn show(notifier: &ToastNotifier, title: &str, body: &str, on_click: &Arc<dyn Fn() + Send + Sync>) -> windows::core::Result<()> {
    let xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        escape(title),
        escape(body)
    );
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    let click = on_click.clone();
    toast.Activated(&TypedEventHandler::new(move |_: Ref<ToastNotification>, _: Ref<IInspectable>| {
        click();
        Ok(())
    }))?;
    notifier.Show(&toast)
}

/// Escapes text for XML element content; drops characters XML cannot carry.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().take(500) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() && c != '\n' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

/// Registers the AUMID's display name and icon for the current user.
fn register(aumid: &str, display_name: &str, icon: Option<&PathBuf>) {
    let key = wide(format!(r"Software\Classes\AppUserModelId\{aumid}"));
    let mut values = vec![("DisplayName", display_name.to_owned())];
    if let Some(icon) = icon {
        values.push(("IconUri", icon.display().to_string()));
    }
    for (name, value) in values {
        let name = wide(name);
        let data = wide(&value);
        let r = unsafe {
            RegSetKeyValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr()), REG_SZ.0, Some(data.as_ptr().cast()), (data.len() * 2) as u32)
        };
        if r != ERROR_SUCCESS {
            log::debug!("cannot register the notification identity: {r:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shows a real toast as the installed app would (`app.moush.desktop`). Check it with
    /// PowerShell: `[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications,
    /// ContentType=WindowsRuntime]::History.GetHistory('app.moush.desktop').Count`.
    #[test]
    #[ignore]
    fn shows_a_toast() {
        let t = Toaster::start("app.moush.desktop", "Moush", None, || {});
        assert!(t.show("Moush test", "If you can read this, notifications work."));
        thread::sleep(std::time::Duration::from_secs(3));
    }

    #[test]
    fn escapes_markup() {
        assert_eq!(escape("a<b>&\"c'\u{7}"), "a&lt;b&gt;&amp;&quot;c&apos; ");
    }
}
