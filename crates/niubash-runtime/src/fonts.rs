//! Nerd Font detection and install recommendations.
//!
//! Download retraction (owner ruling 2026-10-04): the shell carries zero
//! network/HTTP responsibility, so `niu font` no longer downloads or
//! extracts font zips. What remains is detection — scanning the per-user
//! and system font directories plus the registered font lists (HKCU and
//! HKLM on Windows) — and a recommendation surface: on Windows the winget
//! nerd-fonts packages (fonts are not part of wpm's Unix command layer,
//! so the package-manager channel for them is winget/scoop), on other
//! platforms the native package manager or nerdfonts.com. No HTTP, no
//! zip extraction, no registration writes.

use std::path::{Path, PathBuf};

/// A Nerd Font niu knows how to detect and recommend.
pub struct NerdFont {
    /// Menu label shown to the user.
    pub label: &'static str,
    /// Value a Windows Terminal profile's `font.face` should use.
    pub face: &'static str,
    /// winget package id (`winget install --id <id>`), when one exists.
    pub winget_id: Option<&'static str>,
    /// scoop manifest name in the `nerd-fonts` bucket, when one exists.
    pub scoop: Option<&'static str>,
    /// Homebrew cask name (`brew install --cask <name>`).
    pub brew_cask: &'static str,
}

/// Fonts niu detects and recommends (`niu font`, the setup wizard's
/// environment summary, `niu doctor`).
pub const FONT_OPTIONS: &[NerdFont] = &[
    NerdFont {
        label: "JetBrainsMono Nerd Font",
        face: "JetBrainsMono Nerd Font Mono",
        winget_id: Some("DEVCOM.JetBrainsMonoNerdFont"),
        scoop: Some("JetBrainsMono-NF"),
        brew_cask: "font-jetbrains-mono-nerd-font",
    },
    NerdFont {
        label: "MesloLGM Nerd Font",
        face: "MesloLGM Nerd Font Mono",
        // No winget package exists for Meslo; scoop's nerd-fonts bucket
        // and the nerdfonts.com release carry it.
        winget_id: None,
        scoop: Some("Meslo-NF"),
        brew_cask: "font-meslo-lg-nerd-font",
    },
    NerdFont {
        label: "CaskaydiaCove Nerd Font",
        face: "CaskaydiaCove Nerd Font Mono",
        winget_id: None,
        scoop: Some("CascadiaCode-NF"),
        brew_cask: "font-caskaydia-cove-nerd-font",
    },
];

/// Menu labels for the font choice question (plus room for a Skip entry
/// appended by the caller).
pub fn menu_labels() -> Vec<String> {
    FONT_OPTIONS.iter().map(|f| f.label.to_string()).collect()
}

/// The copy-pasteable install commands for one font: winget first on
/// Windows (fonts are a package-manager channel, not wpm's Unix command
/// layer), scoop as the Windows alternative, brew on macOS, and always
/// nerdfonts.com as the channel-free fallback.
fn install_lines(font: &NerdFont) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(id) = font.winget_id {
        lines.push(format!("winget install --id {id}    # Windows"));
    }
    if let Some(name) = font.scoop {
        lines.push(format!(
            "scoop bucket add nerd-fonts; scoop install {name}   # Windows (alternative)"
        ));
    }
    lines.push(format!("brew install --cask {}   # macOS", font.brew_cask));
    lines.push("https://www.nerdfonts.com/font-downloads   # manual download".to_string());
    lines
}

/// True when any installed font name or file looks like a Nerd Font.
/// Checks the per-user and system font directories plus the registry
/// font lists (HKCU and HKLM).
pub fn nerd_font_installed() -> bool {
    font_dirs().iter().any(|dir| dir_has_nerd_font(dir)) || registry_has_nerd_font()
}

/// `niu font` — report Nerd Font detection and print per-font install
/// recommendations. Non-interactive by design (no terminal needed, no
/// network used): the command observes and advises, it never installs.
pub fn run_font_command() -> std::result::Result<(), anyhow::Error> {
    println!(
        "{}",
        crate::text_style::bold("Nerd Fonts — detection & install recommendations")
    );
    println!(
        "{}",
        crate::text_style::dim(
            "  niu no longer downloads fonts (download retraction 2026-10-04); \
             install one with your package manager, then set it in your terminal"
        )
    );
    println!();
    if nerd_font_installed() {
        println!(
            "  {} a Nerd Font is installed — icon themes are unlocked",
            crate::text_style::green("detected:")
        );
    } else {
        println!(
            "  {} no Nerd Font found — icon themes need one",
            crate::text_style::yellow("missing:")
        );
    }
    println!();
    for font in FONT_OPTIONS {
        println!("  {} ('{}' in your terminal)", font.label, font.face);
        for line in install_lines(font) {
            println!("    {line}");
        }
    }
    println!();
    println!(
        "  {}",
        crate::text_style::dim(
            "after installing: set your terminal font to the face above \
             (Windows Terminal: profile → Appearance → Font face)"
        )
    );
    Ok(())
}

// ── Detection ────────────────────────────────────────────────────────────────

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(windir) = std::env::var_os("WINDIR") {
        dirs.push(PathBuf::from(windir).join("Fonts"));
    }
    if let Some(local) = user_fonts_dir() {
        dirs.push(local);
    }
    dirs
}

#[cfg(windows)]
fn user_fonts_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Microsoft").join("Windows").join("Fonts"))
}

/// Per-user font directory on Unix: the classic fontconfig `~/.fonts`,
/// which desktop environments scan without any registration step.
#[cfg(not(windows))]
fn user_fonts_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".fonts"))
}

fn dir_has_nerd_font(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains("nerd")
            })
        })
        .unwrap_or(false)
}

/// Value names under `HKCU`/`HKLM ...\Fonts` — the registered font list.
#[cfg(windows)]
fn font_value_names() -> Vec<String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
        KEY_READ,
    };

    fn to_wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    const FONTS_KEY: &str = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts";
    let mut names = Vec::new();
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let mut key: HKEY = std::ptr::null_mut();
        let opened =
            unsafe { RegOpenKeyExW(root, to_wide(FONTS_KEY).as_ptr(), 0, KEY_READ, &mut key) };
        if opened != ERROR_SUCCESS {
            continue;
        }
        let mut index = 0u32;
        loop {
            let mut buf = vec![0u16; 256];
            let mut len = buf.len() as u32;
            let status = unsafe {
                RegEnumValueW(
                    key,
                    index,
                    buf.as_mut_ptr(),
                    &mut len,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if status != ERROR_SUCCESS {
                break;
            }
            names.push(String::from_utf16_lossy(&buf[..len as usize]));
            index += 1;
        }
        unsafe {
            RegCloseKey(key);
        }
    }
    names
}

#[cfg(windows)]
fn registry_has_nerd_font() -> bool {
    font_value_names()
        .iter()
        .any(|name| name.to_lowercase().contains("nerd"))
}

#[cfg(not(windows))]
fn registry_has_nerd_font() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_options_carry_a_face_and_a_recommendation() {
        for font in FONT_OPTIONS {
            assert!(font.label.contains("Nerd Font"));
            assert!(font.face.contains("Nerd Font"));
            // Every font must recommend at least one channel plus the
            // channel-free fallback (nerdfonts.com is always appended).
            assert!(!install_lines(font).is_empty(), "{}", font.label);
        }
    }

    #[test]
    fn install_lines_are_offline_and_name_the_channels() {
        for font in FONT_OPTIONS {
            let joined = install_lines(font).join("\n");
            assert!(joined.contains("nerdfonts.com"), "{}", font.label);
            if let Some(id) = font.winget_id {
                assert!(
                    joined.contains(&format!("winget install --id {id}")),
                    "{joined}"
                );
            }
        }
    }

    #[test]
    fn menu_labels_cover_options() {
        assert_eq!(menu_labels().len(), FONT_OPTIONS.len());
    }
}
