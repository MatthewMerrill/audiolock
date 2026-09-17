#![windows_subsystem = "windows"]

use com_policy_config::{IPolicyConfig, PolicyConfigClient};
use image::ImageReader;
use std::io::{self, Cursor, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};
use windows::core::{Result, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_core::*;

const APP_VERSION: &str = "2.1.0-tray-toggle";
const ICON_BYTES: &[u8] = include_bytes!("../icon.png");

static IS_UPDATING: AtomicBool = AtomicBool::new(false);
static AUDIO_ENABLED: AtomicBool = AtomicBool::new(true);

fn log(msg: &str) {
    println!("{}", msg);
    let _ = io::stdout().flush();
}

fn log_err(msg: &str) {
    eprintln!("{}", msg);
    let _ = io::stderr().flush();
}

fn load_icon() -> Option<Icon> {
    let image = ImageReader::new(Cursor::new(ICON_BYTES))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;

    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    Icon::from_rgba(rgba.into_raw(), width, height).ok()
}

fn setup_tray(menu: &Menu, enabled_item: &CheckMenuItem, exit_item: &MenuItem) -> Option<tray_icon::TrayIcon> {
    if menu.append(enabled_item).is_err() || menu.append(exit_item).is_err() {
        log_err("[ERROR] Failed to create tray menu items.");
        return None;
    }

    let icon = match load_icon() {
        Some(icon) => icon,
        None => {
            log_err("[ERROR] Failed to decode bundled tray icon.");
            return None;
        }
    };

    match TrayIconBuilder::new()
        .with_tooltip("Audio Lock")
        .with_menu(Box::new(menu.clone()))
        .with_icon(icon)
        .build()
    {
        Ok(tray_icon) => Some(tray_icon),
        Err(err) => {
            log_err(&format!("[ERROR] Failed to build tray icon: {}", err));
            None
        }
    }
}

unsafe fn set_default_communications_device(device_id: PCWSTR) -> Result<()> {
    let device_str = device_id.to_string().unwrap_or_default();
    log(&format!("[INFO] Syncing communications device to: {}", device_str));

    let policy_config: IPolicyConfig = CoCreateInstance(&PolicyConfigClient, None, CLSCTX_ALL)?;
    policy_config.SetDefaultEndpoint(device_id, eCommunications)?;

    log("[SUCCESS] Communications device synced.");
    Ok(())
}

#[implement(IMMNotificationClient)]
struct AudioWatcher;

impl IMMNotificationClient_Impl for AudioWatcher_Impl {
    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        pwstrdefaultdeviceid: &PCWSTR,
    ) -> Result<()> {
        if flow == eRender && role == eMultimedia {
            if !AUDIO_ENABLED.load(Ordering::SeqCst) {
                return Ok(());
            }

            if IS_UPDATING.swap(true, Ordering::SeqCst) {
                return Ok(());
            }

            unsafe {
                let com_init = CoInitializeEx(None, COINIT_MULTITHREADED);

                if let Err(e) = set_default_communications_device(*pwstrdefaultdeviceid) {
                    log_err(&format!("[ERROR] Failed to set default comms device: {:?}", e));
                }

                if com_init.is_ok() {
                    CoUninitialize();
                }
            }

            IS_UPDATING.store(false, Ordering::SeqCst);
        }
        Ok(())
    }

    fn OnDeviceStateChanged(&self, _id: &PCWSTR, _state: DEVICE_STATE) -> Result<()> { Ok(()) }
    fn OnDeviceAdded(&self, _id: &PCWSTR) -> Result<()> { Ok(()) }
    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> Result<()> { Ok(()) }
    fn OnPropertyValueChanged(&self, _id: &PCWSTR, _key: &PROPERTYKEY) -> Result<()> { Ok(()) }
}

fn handle_menu_event(event: MenuEvent, enabled_item: &CheckMenuItem, exit_item: &MenuItem) {
    if event.id == enabled_item.id() {
        let enabled = enabled_item.is_checked();
        AUDIO_ENABLED.store(enabled, Ordering::SeqCst);
        log(&format!("[INFO] Audio sync {}", if enabled { "enabled" } else { "disabled" }));
        return;
    }

    if event.id == exit_item.id() {
        log("[INFO] Exit requested from tray menu.");
        std::process::exit(0);
    }
}

fn main() -> Result<()> {
    log(&format!("=== Audio Comm Sync (v{}) ===", APP_VERSION));

    let menu = Menu::new();
    let enabled_item = CheckMenuItem::new("Enabled", true, true, None);
    let exit_item = MenuItem::new("Exit", true, None);

    let _tray_icon = setup_tray(&menu, &enabled_item, &exit_item);

    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;

        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

        let watcher: IMMNotificationClient = AudioWatcher.into();
        enumerator.RegisterEndpointNotificationCallback(&watcher)?;

        log("[SUCCESS] Listening for audio device changes...");

        let mut msg = MSG::default();
        loop {
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                handle_menu_event(event, &enabled_item, &exit_item);
            }

            if !GetMessageW(&mut msg, Some(HWND::default()), 0, 0).as_bool() {
                break;
            }

            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        enumerator.UnregisterEndpointNotificationCallback(&watcher)?;
        CoUninitialize();
    }
    Ok(())
}