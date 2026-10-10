//! Settings-location discovery and narrowly owned disposable cache maintenance.
use crate::settings::{Paths, Settings, commit, prepare, private_dir};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

const MAX_LOCATOR: u64 = 16 * 1024;
const CACHE_MARKER: &[u8] = b"opterm disposable cache v1\n";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Locator {
    version: u32,
    root: PathBuf,
}
fn root_syntax(root: &Path) -> Result<()> {
    ensure!(root.is_absolute(), "Choose an absolute Opterm folder path");
    ensure!(
        !root
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir)),
        "Opterm folder must not contain parent-directory components"
    );
    Ok(())
}
/// Resolve a remembered location without ever silently adopting another location.
pub fn discover(mut paths: Paths) -> Paths {
    let Some(locator) = &paths.locator else {
        return paths;
    };
    let outcome = (|| -> Result<Option<PathBuf>> {
        let meta = match fs::symlink_metadata(locator) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        ensure!(
            meta.is_file(),
            "Settings-location locator is not a regular file"
        );
        let mut bytes = Vec::new();
        File::open(locator)?
            .take(MAX_LOCATOR + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_LOCATOR,
            "Settings-location locator is too large"
        );
        let locator: Locator = serde_json::from_slice(&bytes)?;
        ensure!(
            locator.version == 1,
            "Unsupported settings-location locator version"
        );
        root_syntax(&locator.root)?;
        Ok(Some(locator.root))
    })();
    match outcome {
        Ok(Some(root)) => {
            paths.config = root.join("settings/settings.toml");
            paths.cache = root.join("cache");
            paths.selected_root = Some(root.clone());
            if !fs::symlink_metadata(&root).is_ok_and(|meta| meta.is_dir()) {
                paths.location_notice = Some(format!(
                    "Selected Opterm folder {} is unavailable. Loaded recovery only; choose an available folder or reconnect it and relaunch. Its location was not changed.",
                    root.display()
                ));
            }
        }
        Ok(None) => {}
        Err(error) => {
            paths.location_notice = Some(format!(
                "Cannot read settings location at {}: {error:#}. Loaded recovery only; choose an Opterm folder to repair discovery. Default settings were not activated.",
                locator.display()
            ));
        }
    }
    paths
}
/// Stage the copy first, then atomically publish its next-launch locator.
pub fn stage(paths: &Paths, settings: &Settings, root: &Path) -> Result<PathBuf> {
    root_syntax(root)?;
    let locator=paths.locator.as_ref().context("A settings-directory environment override is active. Remove OPTERM_CONFIG_DIR to use remembered folder selection.")?;
    if root.exists() {
        ensure!(
            fs::symlink_metadata(root)?.is_dir(),
            "Selected Opterm folder must be a real directory"
        );
    } else {
        private_dir(root)?;
    }
    let destination = root.join("settings/settings.toml");
    let cache = root.join("cache");
    ensure!(
        destination != paths.backup,
        "Settings and recovery must remain separate"
    );
    ensure!(
        !paths.backup.starts_with(&cache),
        "Recovery cannot be placed inside disposable cache"
    );
    let bytes = settings.encode()?;
    if destination.exists() {
        ensure!(
            fs::symlink_metadata(&destination)?.is_file(),
            "Destination settings path is not a regular file"
        );
        let mut existing = Vec::new();
        File::open(&destination)?
            .take(65_537)
            .read_to_end(&mut existing)?;
        ensure!(
            existing == bytes,
            "Destination already contains different settings. Choose an empty Opterm folder; existing files were preserved."
        );
    } else {
        commit(prepare(&destination, &bytes)?, &destination)?;
    }
    private_dir(&cache)?;
    let bytes = serde_json::to_vec_pretty(&Locator {
        version: 1,
        root: root.into(),
    })?;
    ensure!(
        bytes.len() as u64 <= MAX_LOCATOR,
        "Chosen folder path is too long"
    );
    commit(prepare(locator, &bytes)?, locator)?;
    Ok(destination)
}
fn resolved(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    ensure!(
        !absolute
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir)),
        "Storage paths must not contain parent-directory components"
    );
    let mut ancestor = absolute.as_path();
    let mut tail = Vec::new();
    while fs::symlink_metadata(ancestor).is_err() {
        tail.push(
            ancestor
                .file_name()
                .context("Cannot resolve storage path")?
                .to_owned(),
        );
        ancestor = ancestor
            .parent()
            .context("Cannot resolve storage path parent")?;
    }
    let mut canonical = fs::canonicalize(ancestor)?;
    for component in tail.into_iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}
/// Only this marker-owned child is disposable; pre-existing files in cache stay intact.
/// Durable state/history/config/recovery remain completely outside its scope. Returns removed bytes.
pub fn clear_cache(paths: &Paths) -> Result<u64> {
    ensure!(
        paths.location_notice.is_none(),
        "Settings location is unavailable or invalid. Choose an available Opterm folder before clearing cache."
    );
    let owned = paths.cache.join("opterm-disposable");
    let canonical_owned = resolved(&owned)?;
    let state = resolved(&crate::service::state_dir()?)?;
    ensure!(
        !state.starts_with(&canonical_owned) && !canonical_owned.starts_with(&state),
        "Cache overlaps live-session state, saved layouts or output; nothing was removed"
    );
    for preserved in [&paths.config, &paths.backup] {
        ensure!(
            !resolved(preserved)?.starts_with(&canonical_owned),
            "Cache overlaps settings or recovery; nothing was removed"
        );
    }
    private_dir(&paths.cache)?;
    let marker = owned.join(".opterm-cache");
    if !owned.exists() {
        private_dir(&owned)?;
        commit(prepare(&marker, CACHE_MARKER)?, &marker)?;
    }
    private_dir(&owned)?;
    ensure!(
        fs::symlink_metadata(&owned)?.is_dir(),
        "Disposable cache must be a real directory"
    );
    if !marker.exists() {
        ensure!(
            fs::read_dir(&owned)?.next().is_none(),
            "Unrecognized cache folder contains files; nothing was removed"
        );
        commit(prepare(&marker, CACHE_MARKER)?, &marker)?;
    }
    let mut marker_bytes = Vec::new();
    File::open(&marker)?
        .take((CACHE_MARKER.len() + 1) as u64)
        .read_to_end(&mut marker_bytes)?;
    ensure!(
        fs::symlink_metadata(&marker)?.is_file() && marker_bytes == CACHE_MARKER,
        "Unrecognized cache marker; nothing was removed"
    );
    let mut removed = 0;
    for entry in fs::read_dir(&owned)? {
        let path = entry?.path();
        if path == marker {
            continue;
        }
        let meta = fs::symlink_metadata(&path)?;
        // Cached entries are flat files written by Opterm. Unknown names/directories are retained.
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("opterm-"))
            && meta.is_file()
        {
            fs::remove_file(&path)?;
            removed += meta.len();
        }
    }
    Ok(removed)
}
/// Native directory picker; call from a worker so the desktop keeps responding.
pub fn pick_directory() -> Result<Option<PathBuf>> {
    #[cfg(target_os = "linux")]
    let output = match Command::new("zenity")
        .args([
            "--file-selection",
            "--directory",
            "--title=Choose Opterm folder",
        ])
        .output()
    {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Command::new("kdialog")
            .args([
                "--getexistingdirectory",
                ".",
                "--title",
                "Choose Opterm folder",
            ])
            .output()
            .context("Install zenity or kdialog, or enter the folder path directly")?,
        result => result.context("Open folder picker")?,
    };
    #[cfg(target_os = "macos")]
    let output = Command::new("osascript")
        .args([
            "-e",
            "POSIX path of (choose folder with prompt \"Choose Opterm folder\")",
        ])
        .output()
        .context("Open folder picker")?;
    #[cfg(target_os="windows")]
    let output=Command::new("powershell.exe").args(["-NoProfile","-STA","-Command","[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; Add-Type -AssemblyName System.Windows.Forms; $picker=New-Object System.Windows.Forms.FolderBrowserDialog; $picker.Description='Choose Opterm folder'; if($picker.ShowDialog() -eq 'OK'){[Console]::Write($picker.SelectedPath)}"]).output().context("Open folder picker")?;
    if !output.status.success() {
        if output.status.code() == Some(1) && output.stderr.is_empty()
            || String::from_utf8_lossy(&output.stderr).contains("(-128)")
        {
            return Ok(None);
        }
        anyhow::bail!(
            "Folder picker failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let path = String::from_utf8(output.stdout)?
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    Ok((!path.is_empty()).then(|| PathBuf::from(path)))
}
