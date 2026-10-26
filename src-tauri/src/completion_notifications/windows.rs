//! Use a per-user registered identity even when the executable lives in
//! target/release or target/debug. The plugin otherwise selects PowerShell.
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager, Runtime};
use tauri_winrt_notification::{IconCrop, Toast};

const NAME: &str = "WhisperTube";
const ICON: &[u8] = include_bytes!("../../icons/128x128.png");

fn registry_path(identifier: &str) -> Result<String, String> {
    if identifier.is_empty()
        || identifier.len() > 128
        || !identifier
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'))
    {
        return Err("Identitas notifikasi Windows tidak valid.".into());
    }
    Ok(format!(r"SOFTWARE\Classes\AppUserModelId\{identifier}"))
}

fn icon_file(directory: &Path) -> Result<PathBuf, String> {
    crate::durable::create_private_dir_all(directory)
        .map_err(|e| format!("Folder ikon notifikasi: {e}"))?;
    // A changed filename also avoids Windows retaining an old cached icon.
    let digest = format!("{:x}", Sha256::digest(ICON));
    let path = directory.join(format!("notification-{}.png", &digest[..16]));
    match fs::read(&path) {
        Ok(bytes) if bytes == ICON => return Ok(path),
        Ok(_) => return Err("Ikon notifikasi berubah dan gagal verifikasi.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Gagal membaca ikon notifikasi: {error}")),
    }
    let temporary = directory.join(format!(".icon-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        fs::write(&temporary, ICON).map_err(|e| format!("Gagal menulis ikon notifikasi: {e}"))?;
        crate::durable::publish(&temporary, &path)
            .map_err(|e| format!("Gagal memasang ikon notifikasi: {e}"))
    })();
    if result.is_err() {
        if let Err(error) = fs::remove_file(&temporary) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!("Notification icon cleanup: {error}");
            }
        }
    }
    result?;
    Ok(path)
}

fn register(identifier: &str, icon: &Path) -> Result<(), String> {
    let path = registry_path(identifier)?;
    let key = windows_registry::CURRENT_USER
        .create(path)
        .map_err(|e| format!("Registrasi identitas notifikasi: {e}"))?;
    key.set_string("DisplayName", NAME)
        .map_err(|e| format!("Nama notifikasi: {e}"))?;
    key.set_string("IconBackgroundColor", "0")
        .map_err(|e| format!("Latar ikon notifikasi: {e}"))?;
    key.set_string(
        "IconUri",
        icon.to_str().ok_or("Path ikon notifikasi bukan UTF-8.")?,
    )
    .map_err(|e| format!("Ikon notifikasi: {e}"))
}

pub(super) fn send<R: Runtime>(
    app: &AppHandle<R>,
    title: &str,
    message: &str,
    silent: bool,
) -> Result<(), String> {
    let directory = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("Lokasi ikon notifikasi: {e}"))?
        .join("branding");
    let icon = icon_file(&directory)?;
    let identifier = &app.config().identifier;
    register(identifier, &icon)?;
    let toast =
        Toast::new(identifier)
            .title(title)
            .text1(message)
            .icon(&icon, IconCrop::Square, NAME);
    let toast = if silent { toast.sound(None) } else { toast };
    toast
        .show()
        .map_err(|e| format!("Pengiriman notifikasi WhisperTube: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_rejects_registry_path_injection() {
        for id in ["", r"app\other", "app/other", "app\nother"] {
            assert!(registry_path(id).is_err());
        }
        assert!(registry_path("app.whispertube.local")
            .unwrap()
            .ends_with(r"AppUserModelId\app.whispertube.local"));
    }
    #[test]
    fn icon_and_registration_match_the_application_without_sending_a_toast() {
        let root =
            std::env::temp_dir().join(format!("whispertube-notification-{}", uuid::Uuid::new_v4()));
        let id = format!("WhisperTube.Tests.{}", uuid::Uuid::new_v4());
        struct Cleanup {
            root: PathBuf,
            key: String,
        }
        impl Drop for Cleanup {
            fn drop(&mut self) {
                if let Err(e) = windows_registry::CURRENT_USER.remove_tree(&self.key) {
                    eprintln!("Notification test registry cleanup: {e}");
                }
                if let Err(e) = fs::remove_dir_all(&self.root) {
                    eprintln!("Notification test icon cleanup: {e}");
                }
            }
        }
        let _cleanup = Cleanup {
            root: root.clone(),
            key: registry_path(&id).unwrap(),
        };
        let path = icon_file(&root).unwrap();
        assert_eq!(fs::read(&path).unwrap(), ICON);
        assert_eq!(icon_file(&root).unwrap(), path);
        register(&id, &path).unwrap();
        let key = windows_registry::CURRENT_USER
            .open(registry_path(&id).unwrap())
            .unwrap();
        assert_eq!(key.get_string("DisplayName").unwrap(), NAME);
        assert_eq!(key.get_string("IconUri").unwrap(), path.to_str().unwrap());
        fs::write(&path, b"corrupt").unwrap();
        assert!(icon_file(&root).is_err());
    }
}
