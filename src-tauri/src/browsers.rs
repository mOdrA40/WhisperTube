use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::types::{BrowserInfo, BrowserProfile};

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum BrowserKind {
    Chromium,
    Firefox,
    Safari,
}

struct BrowserDefinition {
    id: &'static str,
    label: &'static str,
    kind: BrowserKind,
    roots: Vec<PathBuf>,
}

struct DiscoveredProfile {
    public: BrowserProfile,
    cookie_profile: Option<String>,
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn env_path(variable: &str, parts: &[&str]) -> Option<PathBuf> {
    let mut path = PathBuf::from(env::var_os(variable)?);
    for part in parts {
        path.push(part);
    }
    Some(path)
}

fn definitions() -> Vec<BrowserDefinition> {
    let mut definitions = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let local = env_path("LOCALAPPDATA", &[]);
        let roaming = env_path("APPDATA", &[]);
        if let Some(local) = local {
            definitions.extend([
                BrowserDefinition {
                    id: "chrome",
                    label: "Google Chrome",
                    kind: BrowserKind::Chromium,
                    roots: vec![local.join("Google/Chrome/User Data")],
                },
                BrowserDefinition {
                    id: "edge",
                    label: "Microsoft Edge",
                    kind: BrowserKind::Chromium,
                    roots: vec![local.join("Microsoft/Edge/User Data")],
                },
                BrowserDefinition {
                    id: "brave",
                    label: "Brave",
                    kind: BrowserKind::Chromium,
                    roots: vec![local.join("BraveSoftware/Brave-Browser/User Data")],
                },
                BrowserDefinition {
                    id: "chromium",
                    label: "Chromium",
                    kind: BrowserKind::Chromium,
                    roots: vec![local.join("Chromium/User Data")],
                },
                BrowserDefinition {
                    id: "vivaldi",
                    label: "Vivaldi",
                    kind: BrowserKind::Chromium,
                    roots: vec![local.join("Vivaldi/User Data")],
                },
                BrowserDefinition {
                    id: "whale",
                    label: "Naver Whale",
                    kind: BrowserKind::Chromium,
                    roots: vec![local.join("Naver/Naver Whale/User Data")],
                },
            ]);
        }
        if let Some(roaming) = roaming {
            definitions.push(BrowserDefinition {
                id: "opera",
                label: "Opera",
                kind: BrowserKind::Chromium,
                roots: vec![roaming.join("Opera Software/Opera Stable")],
            });
            definitions.push(BrowserDefinition {
                id: "firefox",
                label: "Mozilla Firefox",
                kind: BrowserKind::Firefox,
                roots: vec![roaming.join("Mozilla/Firefox")],
            });
        }
    }

    #[cfg(target_os = "macos")]
    if let Some(home) = home_dir() {
        let support = home.join("Library/Application Support");
        definitions.extend([
            BrowserDefinition {
                id: "chrome",
                label: "Google Chrome",
                kind: BrowserKind::Chromium,
                roots: vec![support.join("Google/Chrome")],
            },
            BrowserDefinition {
                id: "edge",
                label: "Microsoft Edge",
                kind: BrowserKind::Chromium,
                roots: vec![support.join("Microsoft Edge")],
            },
            BrowserDefinition {
                id: "brave",
                label: "Brave",
                kind: BrowserKind::Chromium,
                roots: vec![support.join("BraveSoftware/Brave-Browser")],
            },
            BrowserDefinition {
                id: "chromium",
                label: "Chromium",
                kind: BrowserKind::Chromium,
                roots: vec![support.join("Chromium")],
            },
            BrowserDefinition {
                id: "opera",
                label: "Opera",
                kind: BrowserKind::Chromium,
                roots: vec![support.join("com.operasoftware.Opera")],
            },
            BrowserDefinition {
                id: "vivaldi",
                label: "Vivaldi",
                kind: BrowserKind::Chromium,
                roots: vec![support.join("Vivaldi")],
            },
            BrowserDefinition {
                id: "whale",
                label: "Naver Whale",
                kind: BrowserKind::Chromium,
                roots: vec![support.join("Naver/Whale")],
            },
            BrowserDefinition {
                id: "firefox",
                label: "Mozilla Firefox",
                kind: BrowserKind::Firefox,
                roots: vec![support.join("Firefox")],
            },
            BrowserDefinition {
                id: "safari",
                label: "Safari",
                kind: BrowserKind::Safari,
                roots: vec![home.join("Library/Safari")],
            },
        ]);
    }

    #[cfg(target_os = "linux")]
    if let Some(config) =
        env_path("XDG_CONFIG_HOME", &[]).or_else(|| home_dir().map(|home| home.join(".config")))
    {
        definitions.extend([
            BrowserDefinition {
                id: "chrome",
                label: "Google Chrome",
                kind: BrowserKind::Chromium,
                roots: vec![config.join("google-chrome")],
            },
            BrowserDefinition {
                id: "edge",
                label: "Microsoft Edge",
                kind: BrowserKind::Chromium,
                roots: vec![config.join("microsoft-edge")],
            },
            BrowserDefinition {
                id: "brave",
                label: "Brave",
                kind: BrowserKind::Chromium,
                roots: vec![config.join("BraveSoftware/Brave-Browser")],
            },
            BrowserDefinition {
                id: "chromium",
                label: "Chromium",
                kind: BrowserKind::Chromium,
                roots: vec![config.join("chromium")],
            },
            BrowserDefinition {
                id: "opera",
                label: "Opera",
                kind: BrowserKind::Chromium,
                roots: vec![config.join("opera")],
            },
            BrowserDefinition {
                id: "vivaldi",
                label: "Vivaldi",
                kind: BrowserKind::Chromium,
                roots: vec![config.join("vivaldi")],
            },
            BrowserDefinition {
                id: "whale",
                label: "Naver Whale",
                kind: BrowserKind::Chromium,
                roots: vec![config.join("naver-whale")],
            },
        ]);
    }
    if let Some(home) = home_dir() {
        let firefox_root = home.join(".mozilla/firefox");
        if !definitions
            .iter()
            .any(|definition| definition.id == "firefox")
        {
            definitions.push(BrowserDefinition {
                id: "firefox",
                label: "Mozilla Firefox",
                kind: BrowserKind::Firefox,
                roots: vec![firefox_root],
            });
        }
    }

    definitions
}

fn first_existing_root(definition: &BrowserDefinition) -> Option<PathBuf> {
    definition.roots.iter().find(|root| root.is_dir()).cloned()
}

fn chromium_label(root: &Path, profile_id: &str) -> Option<String> {
    let local_state = fs::read_to_string(root.join("Local State")).ok()?;
    let value: Value = serde_json::from_str(&local_state).ok()?;
    value
        .get("profile")?
        .get("info_cache")?
        .get(profile_id)?
        .get("name")?
        .as_str()
        .map(str::to_string)
}

fn chromium_profiles(root: &Path) -> Vec<DiscoveredProfile> {
    let mut profiles = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return profiles,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let id = entry.file_name().to_string_lossy().to_string();
        if !path.is_dir() || !(id == "Default" || id.starts_with("Profile ")) {
            continue;
        }
        profiles.push(DiscoveredProfile {
            public: BrowserProfile {
                id: id.clone(),
                label: chromium_label(root, &id).unwrap_or_else(|| id.clone()),
                is_default: id == "Default",
            },
            cookie_profile: Some(id),
        });
    }
    if profiles.is_empty()
        && (root.join("Cookies").exists() || root.join("Network/Cookies").exists())
    {
        profiles.push(DiscoveredProfile {
            public: BrowserProfile {
                id: String::new(),
                label: "Default".into(),
                is_default: true,
            },
            cookie_profile: None,
        });
    }
    profiles.sort_by_key(|profile| {
        (
            !profile.public.is_default,
            profile.public.label.to_lowercase(),
        )
    });
    profiles
}

fn add_firefox_profile(
    profiles: &mut Vec<DiscoveredProfile>,
    root: &Path,
    name: Option<String>,
    path_value: String,
    is_relative: bool,
    is_default: bool,
) {
    let profile_path = if is_relative {
        root.join(&path_value)
    } else {
        PathBuf::from(&path_value)
    };
    if !profile_path.is_dir() {
        return;
    }
    let id = format!("profile-{}", profiles.len());
    let label = name.unwrap_or_else(|| path_value.clone());
    profiles.push(DiscoveredProfile {
        public: BrowserProfile {
            id,
            label,
            is_default,
        },
        cookie_profile: Some(profile_path.to_string_lossy().to_string()),
    });
}

fn firefox_profiles(root: &Path) -> Vec<DiscoveredProfile> {
    let contents = fs::read_to_string(root.join("profiles.ini")).unwrap_or_default();
    let mut profiles = Vec::new();
    let mut name: Option<String> = None;
    let mut path: Option<String> = None;
    let mut is_relative = true;
    let mut is_default = false;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            if let Some(path_value) = path.take() {
                add_firefox_profile(
                    &mut profiles,
                    root,
                    name.take(),
                    path_value,
                    is_relative,
                    is_default,
                );
            }
            is_relative = true;
            is_default = false;
        } else if let Some(value) = line.strip_prefix("Name=") {
            name = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("Path=") {
            path = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("IsRelative=") {
            is_relative = value != "0";
        } else if let Some(value) = line.strip_prefix("Default=") {
            is_default = value == "1";
        }
    }
    if let Some(path_value) = path.take() {
        add_firefox_profile(
            &mut profiles,
            root,
            name.take(),
            path_value,
            is_relative,
            is_default,
        );
    }

    if profiles.is_empty() {
        let profiles_dir = root.join("Profiles");
        if let Ok(entries) = fs::read_dir(profiles_dir) {
            for entry in entries.flatten().filter(|entry| entry.path().is_dir()) {
                let is_default = profiles.is_empty();
                add_firefox_profile(
                    &mut profiles,
                    root,
                    Some(entry.file_name().to_string_lossy().into()),
                    entry.path().to_string_lossy().into(),
                    false,
                    is_default,
                );
            }
        }
    }
    if !profiles.is_empty() && !profiles.iter().any(|profile| profile.public.is_default) {
        profiles[0].public.is_default = true;
    }
    profiles.sort_by_key(|profile| {
        (
            !profile.public.is_default,
            profile.public.label.to_lowercase(),
        )
    });
    profiles
}

fn discover_internal() -> Vec<(BrowserInfo, Vec<DiscoveredProfile>)> {
    definitions()
        .into_iter()
        .filter_map(|definition| {
            let root = first_existing_root(&definition)?;
            let profiles = match definition.kind {
                BrowserKind::Chromium => chromium_profiles(&root),
                BrowserKind::Firefox => firefox_profiles(&root),
                BrowserKind::Safari => vec![DiscoveredProfile {
                    public: BrowserProfile {
                        id: String::new(),
                        label: "Safari session".into(),
                        is_default: true,
                    },
                    cookie_profile: None,
                }],
            };
            if profiles.is_empty() {
                return None;
            }
            let public_profiles = profiles
                .iter()
                .map(|profile| profile.public.clone())
                .collect();
            Some((
                BrowserInfo {
                    id: definition.id.into(),
                    label: definition.label.into(),
                    profiles: public_profiles,
                },
                profiles,
            ))
        })
        .collect()
}

pub fn discover_browsers() -> Vec<BrowserInfo> {
    discover_internal()
        .into_iter()
        .map(|(browser, _)| browser)
        .collect()
}

pub fn browser_args(browser: &str, profile: Option<&str>) -> Result<Vec<String>, String> {
    if browser == "none" || browser.is_empty() {
        return Ok(vec![]);
    }
    let discovered = discover_internal();
    let selected = discovered
        .iter()
        .find(|(info, _)| info.id == browser)
        .ok_or_else(|| "Browser session tidak ditemukan di komputer ini.".to_string())?;
    let token = if let Some(profile_id) = profile.filter(|profile| !profile.is_empty()) {
        let profile = selected
            .1
            .iter()
            .find(|profile| profile.public.id == profile_id)
            .ok_or_else(|| "Profile browser yang dipilih tidak ditemukan.".to_string())?;
        format!(
            "{browser}:{}",
            profile.cookie_profile.as_deref().unwrap_or(profile_id)
        )
    } else {
        browser.to_string()
    };
    Ok(vec!["--cookies-from-browser".into(), token])
}
