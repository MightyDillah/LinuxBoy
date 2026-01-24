use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box, Button, CheckButton, Dialog, Entry, FileChooserAction,
    FileChooserNative, FileFilter, Image, Label, ListBox, ListBoxRow, Orientation, ResponseType,
    ScrolledWindow, SelectionMode,
};
use relm4::{Component, ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent};
use relm4::component::{ComponentController, Controller};

use crate::core::capsule::{Capsule, CapsuleMetadata, InstallState};
use crate::core::runtime_manager::RuntimeManager;
use crate::core::system_checker::{SystemCheck, SystemStatus};
use crate::core::umu_database::{UmuDatabase, UmuEntry};
use crate::ui::system_setup_dialog::{SystemSetupDialog, SystemSetupMsg, SystemSetupOutput};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, io, thread};
use walkdir::WalkDir;

#[derive(Debug)]
pub enum MainWindowMsg {
    LoadCapsules,
    OpenAddGame,
    AddGameModeChosen(AddGameMode),
    OpenSystemSetup,
    GamePathSelected(PathBuf),
    AddGameCancelled,
    ExistingSourceFolderSelected(PathBuf),
    ExistingSourceFolderCancelled,
    ExistingGameLocationConfirmed(String),
    ExistingGameLocationCancelled,
    GameNameConfirmed(String),
    InstallerStarted {
        capsule_dir: PathBuf,
        pgid: i32,
    },
    InstallerFinished {
        capsule_dir: PathBuf,
        success: bool,
    },
    UmuDatabaseLoaded(Vec<UmuEntry>),
    UmuDatabaseFailed(String),
    UmuMatchChosen {
        game_id: Option<String>,
        store: Option<String>,
    },
    UmuMatchDialogClosed,
    SaveGameSettings {
        capsule_dir: PathBuf,
        exe_path: String,
        game_id: Option<String>,
        store: Option<String>,
        install_vcredist: bool,
        install_dxweb: bool,
        protonfixes_disable: bool,
        xalia_enabled: bool,
        protonfixes_tricks: Vec<String>,
        protonfixes_replace_cmds: Vec<String>,
        protonfixes_dxvk_sets: Vec<String>,
    },
    SettingsDialogClosed,
    DependenciesSelected {
        capsule_dir: PathBuf,
        install_vcredist: bool,
        install_dxweb: bool,
        force: bool,
    },
    DependenciesFinished {
        capsule_dir: PathBuf,
        installed: Vec<String>,
    },
    DependenciesDialogClosed,
    GameStarted {
        capsule_dir: PathBuf,
        pgid: i32,
    },
    GameFinished {
        capsule_dir: PathBuf,
        success: bool,
    },
    LaunchGame(PathBuf),
    EditGame(PathBuf),
    DeleteGame(PathBuf),
    ResumeInstall(PathBuf),
    KillInstall(PathBuf),
    MarkInstallComplete(PathBuf),
    SystemSetupOutput(SystemSetupOutput),
}

pub struct MainWindow {
    capsules: Vec<Capsule>,
    games_dir: PathBuf,
    system_check: SystemCheck,
    system_setup_dialog: Option<Controller<SystemSetupDialog>>,
    runtime_mgr: RuntimeManager,
    add_game_dialog: Option<Dialog>,
    game_path_dialog: Option<FileChooserNative>,
    name_dialog: Option<Dialog>,
    settings_dialog: Option<Dialog>,
    umu_match_dialog: Option<Dialog>,
    dependency_dialog: Option<Dialog>,
    existing_location_dialog: Option<Dialog>,
    pending_add_mode: Option<AddGameMode>,
    pending_game_path: Option<PathBuf>,
    pending_source_folder: Option<PathBuf>,
    pending_game_name: Option<String>,
    pending_game_id: Option<String>,
    pending_store: Option<String>,
    pending_settings_capsule: Option<PathBuf>,
    active_installs: HashMap<PathBuf, i32>,
    active_games: HashMap<PathBuf, i32>,
    preparing_installs: HashSet<PathBuf>,
    dependency_installs: HashSet<PathBuf>,
    umu_entries: Vec<UmuEntry>,
    umu_loaded: bool,
    umu_load_error: Option<String>,
    games_list: Box,
    library_count_label: Label,
    root_window: ApplicationWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AddGameMode {
    Installer,
    Existing,
}

#[derive(Debug, Clone)]
struct UmuMatch {
    entry: UmuEntry,
    score: i32,
}

#[derive(Debug, Clone)]
struct ExecutableGuess {
    path: PathBuf,
    shortcut: Option<PathBuf>,
    score: i32,
}

impl MainWindow {
    const DEP_VCREDIST: &'static str = "vcredist";
    const DEP_DXWEB: &'static str = "dxweb";

    fn normalize_words(value: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut current = String::new();
        for ch in value.chars() {
            if ch.is_ascii_alphanumeric() {
                current.push(ch.to_ascii_lowercase());
            } else if !current.is_empty() {
                words.push(current);
                current = String::new();
            }
        }
        if !current.is_empty() {
            words.push(current);
        }
        words
    }

    fn score_match(input: &str, candidate: &str) -> Option<i32> {
        let input_compact = UmuDatabase::normalize_title(input);
        let candidate_compact = UmuDatabase::normalize_title(candidate);
        if input_compact.is_empty() || candidate_compact.is_empty() {
            return None;
        }

        if candidate_compact == input_compact {
            return Some(0);
        }

        if candidate_compact.contains(&input_compact) {
            return Some(1);
        }

        let input_words = Self::normalize_words(input);
        let candidate_words = Self::normalize_words(candidate);
        if !input_words.is_empty()
            && input_words
                .iter()
                .all(|word| candidate_words.contains(word))
        {
            return Some(2);
        }

        if input_compact.contains(&candidate_compact) && candidate_compact.len() >= 4 {
            return Some(3);
        }

        None
    }

    fn score_acronym(input: &str, acronym: &str) -> Option<i32> {
        let input_compact = UmuDatabase::normalize_title(input);
        let acronym_compact = UmuDatabase::normalize_title(acronym);
        if input_compact.is_empty() || acronym_compact.is_empty() {
            return None;
        }

        if input_compact == acronym_compact {
            return Some(0);
        }

        if input_compact.contains(&acronym_compact) && acronym_compact.len() >= 3 {
            return Some(2);
        }

        None
    }

    fn parse_list_input(value: &str) -> Vec<String> {
        value
            .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn is_exe_file(path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("exe"))
            .unwrap_or(false)
    }

    fn compact_name(value: &str) -> String {
        UmuDatabase::normalize_title(value)
    }

    fn path_contains_case_insensitive(path: &Path, needle: &str) -> bool {
        let lowered = path.to_string_lossy().to_ascii_lowercase();
        lowered.contains(&needle.to_ascii_lowercase())
    }

    fn is_ignored_exe(path: &Path) -> bool {
        let name = path
            .file_name()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let blocked_exact = [
            "uninstall.exe",
            "uninstaller.exe",
            "setup.exe",
            "dxsetup.exe",
            "explorer.exe",
            "rundll32.exe",
            "cmd.exe",
            "powershell.exe",
            "iexplore.exe",
            "chrome.exe",
            "msedge.exe",
            "firefox.exe",
            "opera.exe",
            "brave.exe",
        ];
        if blocked_exact.iter().any(|blocked| name == *blocked) {
            return true;
        }
        if name.starts_with("unins") || name.contains("uninstall") {
            return true;
        }
        if name.contains("redist") || name.contains("vcredist") {
            return true;
        }
        for component in path.components() {
            if let std::path::Component::Normal(value) = component {
                if value.eq_ignore_ascii_case("windows")
                    || value.eq_ignore_ascii_case("system32")
                    || value.eq_ignore_ascii_case("syswow64")
                {
                    return true;
                }
            }
        }
        false
    }

    fn score_exe_candidate(
        path: &Path,
        shortcut: Option<&Path>,
        capsule_name: &str,
        game_dir: Option<&Path>,
    ) -> i32 {
        let mut score = 0;
        if path.is_file() {
            score += 100;
        }
        if shortcut.is_some() {
            score += 20;
        }

        let name_compact = Self::compact_name(capsule_name);
        let exe_name = path
            .file_stem()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default();
        let exe_compact = Self::compact_name(&exe_name);
        if !name_compact.is_empty() && !exe_compact.is_empty() {
            if exe_compact == name_compact {
                score += 40;
            } else if exe_compact.contains(&name_compact) {
                score += 25;
            }
        }

        if let Some(shortcut_path) = shortcut {
            let shortcut_name = shortcut_path
                .file_stem()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default();
            let shortcut_compact = Self::compact_name(&shortcut_name);
            if !name_compact.is_empty() && !shortcut_compact.is_empty() {
                if shortcut_compact == name_compact {
                    score += 25;
                } else if shortcut_compact.contains(&name_compact) {
                    score += 15;
                }
            }
        }

        if let Some(game_root) = game_dir {
            if path.starts_with(game_root) {
                score += 30;
            }
        }

        let lowered_path = path.to_string_lossy().to_ascii_lowercase();
        for bad in ["unins", "uninstall", "dxsetup", "directx", "vcredist", "redist"] {
            if lowered_path.contains(bad) {
                score -= 80;
            }
        }
        for bad in ["setup", "installer", "support", "helper", "crash"] {
            if lowered_path.contains(bad) {
                score -= 25;
            }
        }
        for bad in ["launcher", "patcher", "updater"] {
            if lowered_path.contains(bad) {
                score -= 15;
            }
        }

        score
    }

    fn windows_path_to_host(prefix_path: &Path, windows_path: &str) -> Option<PathBuf> {
        let trimmed = windows_path.trim_matches('"').trim();
        if trimmed.len() < 3 {
            return None;
        }
        let normalized = trimmed.replace('/', "\\");
        let lowered = normalized.to_ascii_lowercase();
        if !lowered.starts_with("c:\\") {
            return None;
        }
        let relative = &normalized[3..];
        let sep = std::path::MAIN_SEPARATOR.to_string();
        let host_rel = relative.replace('\\', &sep);
        Some(prefix_path.join("drive_c").join(host_rel))
    }

    fn extract_windows_paths_from_text(text: &str) -> Vec<String> {
        let mut results = Vec::new();
        let bytes = text.as_bytes();
        if bytes.len() < 4 {
            return results;
        }
        let mut i = 0;
        while i + 2 < bytes.len() {
            if bytes[i].is_ascii_alphabetic() && bytes[i + 1] == b':' && bytes[i + 2] == b'\\' {
                let mut end = i + 3;
                while end < bytes.len() {
                    let b = bytes[end];
                    if b.is_ascii_alphanumeric()
                        || matches!(b, b'\\' | b'/' | b'.' | b'_' | b'-' | b' ' | b'(' | b')')
                    {
                        end += 1;
                    } else {
                        break;
                    }
                }
                let candidate = &text[i..end];
                let lower = candidate.to_ascii_lowercase();
                if let Some(idx) = lower.rfind(".exe") {
                    let trimmed = candidate[..idx + 4].to_string();
                    results.push(trimmed);
                }
                i = end;
            } else {
                i += 1;
            }
        }
        results
    }

    fn extract_ascii_sequences(bytes: &[u8]) -> Vec<String> {
        let mut sequences = Vec::new();
        let mut current: Vec<u8> = Vec::new();
        for &b in bytes {
            if b.is_ascii_graphic() || b == b' ' {
                current.push(b);
            } else {
                if current.len() >= 6 {
                    sequences.push(String::from_utf8_lossy(&current).to_string());
                }
                current.clear();
            }
        }
        if current.len() >= 6 {
            sequences.push(String::from_utf8_lossy(&current).to_string());
        }
        sequences
    }

    fn extract_utf16le_sequences(bytes: &[u8]) -> Vec<String> {
        let mut sequences = Vec::new();
        let mut current: Vec<u8> = Vec::new();
        let mut i = 0;
        while i + 1 < bytes.len() {
            let b0 = bytes[i];
            let b1 = bytes[i + 1];
            if b1 == 0 && (b0 == 0 || b0.is_ascii_graphic() || b0 == b' ') {
                if b0 == 0 {
                    if current.len() >= 6 {
                        sequences.push(String::from_utf8_lossy(&current).to_string());
                    }
                    current.clear();
                } else {
                    current.push(b0);
                }
                i += 2;
            } else {
                if current.len() >= 6 {
                    sequences.push(String::from_utf8_lossy(&current).to_string());
                }
                current.clear();
                i += 1;
            }
        }
        if current.len() >= 6 {
            sequences.push(String::from_utf8_lossy(&current).to_string());
        }
        sequences
    }

    fn extract_windows_exe_paths_from_lnk(path: &Path) -> Vec<String> {
        let mut results = Vec::new();
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) => return results,
        };
        for seq in Self::extract_ascii_sequences(&bytes)
            .into_iter()
            .chain(Self::extract_utf16le_sequences(&bytes))
        {
            results.extend(Self::extract_windows_paths_from_text(&seq));
        }
        results.sort();
        results.dedup();
        results
    }

    fn lnk_contains_http_url(path: &Path) -> bool {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) => return false,
        };
        for seq in Self::extract_ascii_sequences(&bytes)
            .into_iter()
            .chain(Self::extract_utf16le_sequences(&bytes))
        {
            let lowered = seq.to_ascii_lowercase();
            if lowered.contains("http://") || lowered.contains("https://") {
                return true;
            }
        }
        false
    }

    fn collect_shortcuts(prefix_path: &Path) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        let users_dir = prefix_path.join("drive_c").join("users");
        if let Ok(entries) = fs::read_dir(&users_dir) {
            for entry in entries.flatten() {
                let user_dir = entry.path();
                if !user_dir.is_dir() {
                    continue;
                }
                roots.push(user_dir.join("Desktop"));
                roots.push(user_dir.join("Start Menu"));
                roots.push(user_dir.join("Start Menu").join("Programs"));
            }
        }

        let mut shortcuts = Vec::new();
        for root in roots {
            if !root.is_dir() {
                continue;
            }
            for entry in WalkDir::new(&root).max_depth(6).follow_links(false) {
                if let Ok(entry) = entry {
                    if entry.file_type().is_file() && Self::is_exe_file(entry.path()) == false {
                        if entry
                            .path()
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .map(|ext| ext.eq_ignore_ascii_case("lnk"))
                            .unwrap_or(false)
                        {
                            shortcuts.push(entry.path().to_path_buf());
                        }
                    }
                }
            }
        }
        shortcuts
    }

    fn find_exe_from_shortcuts(
        prefix_path: &Path,
        capsule_name: &str,
        game_dir: Option<&Path>,
    ) -> Vec<ExecutableGuess> {
        let mut candidates = Vec::new();
        for shortcut in Self::collect_shortcuts(prefix_path) {
            if Self::lnk_contains_http_url(&shortcut) {
                continue;
            }
            for windows_path in Self::extract_windows_exe_paths_from_lnk(&shortcut) {
                if let Some(host_path) = Self::windows_path_to_host(prefix_path, &windows_path) {
                    if host_path.is_file()
                        && Self::is_exe_file(&host_path)
                        && !Self::is_ignored_exe(&host_path)
                    {
                        let score =
                            Self::score_exe_candidate(&host_path, Some(&shortcut), capsule_name, game_dir);
                        candidates.push(ExecutableGuess {
                            path: host_path,
                            shortcut: Some(shortcut.clone()),
                            score,
                        });
                    }
                }
            }
        }
        candidates
    }

    fn find_exe_from_dirs(
        prefix_path: &Path,
        capsule_name: &str,
        game_dir: Option<&Path>,
    ) -> Vec<ExecutableGuess> {
        let mut roots = Vec::new();
        if let Some(game_root) = game_dir {
            roots.push(game_root.to_path_buf());
        }
        let drive_c = prefix_path.join("drive_c");
        roots.push(drive_c.join("Program Files"));
        roots.push(drive_c.join("Program Files (x86)"));
        roots.push(drive_c.join("GOG Games"));
        roots.push(drive_c.join("Games"));

        if roots.iter().all(|root| !root.is_dir()) {
            roots.clear();
            roots.push(drive_c);
        }

        let mut candidates = Vec::new();
        for root in roots {
            if !root.is_dir() {
                continue;
            }
            let walker = WalkDir::new(&root)
                .max_depth(6)
                .follow_links(false)
                .into_iter()
                .filter_entry(|entry| !Self::path_contains_case_insensitive(entry.path(), "windows"));
            for entry in walker.flatten() {
                if entry.file_type().is_file()
                    && Self::is_exe_file(entry.path())
                    && !Self::is_ignored_exe(entry.path())
                {
                    let score =
                        Self::score_exe_candidate(entry.path(), None, capsule_name, game_dir);
                    candidates.push(ExecutableGuess {
                        path: entry.path().to_path_buf(),
                        shortcut: None,
                        score,
                    });
                }
            }
        }
        candidates
    }

    fn guess_executable(capsule: &Capsule) -> Option<ExecutableGuess> {
        let prefix_path = capsule
            .capsule_dir
            .join(format!("{}.AppImage.home", capsule.name))
            .join("prefix");
        let game_dir = capsule
            .metadata
            .game_dir
            .as_deref()
            .map(PathBuf::from)
            .filter(|path| path.is_dir());

        let mut candidates =
            Self::find_exe_from_shortcuts(&prefix_path, &capsule.name, game_dir.as_deref());
        if candidates.is_empty() {
            candidates =
                Self::find_exe_from_dirs(&prefix_path, &capsule.name, game_dir.as_deref());
        }
        candidates.sort_by(|a, b| b.score.cmp(&a.score));
        candidates.into_iter().next()
    }

    fn vcredist_cache_path() -> PathBuf {
        SystemCheck::vcredist_cache_path()
    }

    fn dxweb_cache_path() -> PathBuf {
        SystemCheck::dxweb_cache_path()
    }

    fn is_dependency_installed(metadata: &CapsuleMetadata, dep: &str) -> bool {
        metadata
            .redistributables_installed
            .iter()
            .any(|item| item == dep)
    }

    fn should_prompt_dependencies(&self, metadata: &CapsuleMetadata) -> bool {
        let wants_vcredist = metadata.install_vcredist;
        let wants_dxweb = metadata.install_dxweb;
        if !wants_vcredist && !wants_dxweb {
            return false;
        }
        let vcredist_pending =
            wants_vcredist && !Self::is_dependency_installed(metadata, Self::DEP_VCREDIST);
        let dxweb_pending =
            wants_dxweb && !Self::is_dependency_installed(metadata, Self::DEP_DXWEB);
        vcredist_pending || dxweb_pending
    }

    fn resolve_relative_game_folder(name: &str, input: &str) -> String {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return name.to_string();
        }
        let path = Path::new(trimmed);
        if path.is_absolute() {
            eprintln!("Game folder must be relative to prefix/games. Using default.");
            return name.to_string();
        }
        trimmed.to_string()
    }

    fn unique_path(path: PathBuf) -> PathBuf {
        if !path.exists() {
            return path;
        }
        let parent = path.parent().unwrap_or_else(|| Path::new(""));
        let stem = path
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "game".to_string());
        for idx in 1..1000 {
            let candidate = parent.join(format!("{}-{}", stem, idx));
            if !candidate.exists() {
                return candidate;
            }
        }
        path
    }

    fn copy_dir_recursive(src: &Path, dest: &Path) -> io::Result<()> {
        if !dest.exists() {
            fs::create_dir_all(dest)?;
        }
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let from = entry.path();
            let to = dest.join(entry.file_name());
            if file_type.is_dir() {
                Self::copy_dir_recursive(&from, &to)?;
            } else {
                fs::copy(&from, &to)?;
            }
        }
        Ok(())
    }

    fn update_library_labels(&self) {
        self.library_count_label
            .set_label(&format!("{} games", self.capsules.len()));
    }

    fn sanitize_name(name: &str) -> String {
        name.trim()
            .replace(['/', '\\'], "_")
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
    }

    fn is_generic_installer_stem(stem: &str) -> bool {
        let normalized = Self::compact_name(stem);
        if normalized.is_empty() {
            return true;
        }
        let generic = [
            "setup",
            "installer",
            "install",
            "gog",
            "gogsetup",
            "goginstaller",
            "setupx64",
            "setupx86",
            "update",
            "patch",
        ];
        if generic.contains(&normalized.as_str()) {
            return true;
        }
        if normalized.starts_with("setup") && normalized.len() <= 10 {
            return true;
        }
        false
    }

    fn is_generic_container_name(name: &str) -> bool {
        let normalized = Self::compact_name(name);
        let generic = [
            "downloads",
            "download",
            "desktop",
            "installers",
            "installer",
            "setup",
        ];
        generic.contains(&normalized.as_str())
    }

    fn default_game_name_for_path(mode: AddGameMode, path: &Path) -> Option<String> {
        let stem = path
            .file_stem()
            .map(|value| value.to_string_lossy().to_string())
            .filter(|value| !value.trim().is_empty());
        let parent = path
            .parent()
            .and_then(|dir| dir.file_name())
            .map(|value| value.to_string_lossy().to_string())
            .filter(|value| !value.trim().is_empty());

        let parent_ok = parent
            .as_deref()
            .map(|value| !Self::is_generic_container_name(value))
            .unwrap_or(false);

        let candidate = match mode {
            AddGameMode::Existing => {
                if parent_ok {
                    parent
                } else {
                    stem
                }
            }
            AddGameMode::Installer => {
                if parent_ok {
                    parent
                } else {
                    stem.filter(|value| !Self::is_generic_installer_stem(value))
                }
            }
        };

        candidate
            .map(|value| Self::sanitize_name(&value))
            .filter(|value| !value.trim().is_empty())
    }

    fn unique_game_dir(&self, base_name: &str) -> PathBuf {
        let base = self.games_dir.join(base_name);
        if !base.exists() {
            return base;
        }

        for idx in 1..1000 {
            let candidate = self.games_dir.join(format!("{}-{}", base_name, idx));
            if !candidate.exists() {
                return candidate;
            }
        }

        base
    }

    fn has_command(cmd: &str) -> bool {
        Command::new("which")
            .arg(cmd)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn open_add_game_dialog(&mut self, sender: ComponentSender<Self>) {
        if self.add_game_dialog.is_some() {
            return;
        }

        let dialog = Dialog::builder()
            .title("Add Game")
            .modal(true)
            .transient_for(&self.root_window)
            .build();
        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("Install from installer", ResponseType::Accept);
        dialog.add_button("Add existing game", ResponseType::Apply);

        let content = dialog.content_area();
        let layout = Box::new(Orientation::Vertical, 8);
        layout.set_margin_all(12);

        let title = Label::new(Some("Choose how to add this game"));
        title.set_halign(gtk4::Align::Start);
        title.set_css_classes(&["section-title"]);

        let hint = Label::new(Some(
            "Installers run through UMU. Existing games will be copied into the prefix.",
        ));
        hint.set_halign(gtk4::Align::Start);
        hint.set_wrap(true);
        hint.set_css_classes(&["muted"]);

        layout.append(&title);
        layout.append(&hint);
        content.append(&layout);

        let sender_clone = sender.clone();
        let handled = Rc::new(Cell::new(false));
        let handled_clone = handled.clone();
        dialog.connect_response(move |dialog, response| {
            if handled_clone.replace(true) {
                return;
            }
            match response {
                ResponseType::Accept => {
                    sender_clone.input(MainWindowMsg::AddGameModeChosen(AddGameMode::Installer));
                }
                ResponseType::Apply => {
                    sender_clone.input(MainWindowMsg::AddGameModeChosen(AddGameMode::Existing));
                }
                _ => {
                    sender_clone.input(MainWindowMsg::AddGameCancelled);
                }
            }
            dialog.close();
        });

        dialog.show();
        self.add_game_dialog = Some(dialog);
    }

    fn open_game_path_dialog(&mut self, sender: ComponentSender<Self>, mode: AddGameMode) {
        if let Some(dialog) = &self.game_path_dialog {
            dialog.show();
            return;
        }

        let title = match mode {
            AddGameMode::Installer => "Select Installer",
            AddGameMode::Existing => "Select Game Executable",
        };
        let dialog = FileChooserNative::builder()
            .title(title)
            .action(FileChooserAction::Open)
            .accept_label("Select")
            .cancel_label("Cancel")
            .transient_for(&self.root_window)
            .build();

        let filter = FileFilter::new();
        filter.add_suffix("exe");
        if mode == AddGameMode::Installer {
            filter.add_suffix("msi");
            filter.set_name(Some("Windows installers (.exe, .msi)"));
        } else {
            filter.set_name(Some("Windows executables (.exe)"));
        }
        dialog.add_filter(&filter);

        let sender_clone = sender.clone();
        let handled = Rc::new(Cell::new(false));
        let handled_clone = handled.clone();
        dialog.connect_response(move |dialog, response| {
            if handled_clone.replace(true) {
                return;
            }
            if response == ResponseType::Accept {
                if let Some(file) = dialog.file() {
                    if let Some(path) = file.path() {
                        sender_clone.input(MainWindowMsg::GamePathSelected(path));
                    } else {
                        sender_clone.input(MainWindowMsg::AddGameCancelled);
                    }
                } else {
                    sender_clone.input(MainWindowMsg::AddGameCancelled);
                }
            } else {
                sender_clone.input(MainWindowMsg::AddGameCancelled);
            }

            dialog.destroy();
        });

        dialog.show();
        self.game_path_dialog = Some(dialog);
    }

    fn open_name_dialog(&mut self, sender: ComponentSender<Self>) {
        if self.name_dialog.is_some() {
            return;
        }

        let dialog = Dialog::builder()
            .title("Game Name")
            .modal(true)
            .transient_for(&self.root_window)
            .build();
        dialog.set_default_width(420);
        dialog.set_default_height(180);
        dialog.set_resizable(false);
        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("Create", ResponseType::Accept);
        dialog.set_default_response(ResponseType::Accept);

        let content = dialog.content_area();
        content.set_margin_all(16);
        content.set_spacing(10);
        let label = Label::new(Some("Name your game"));
        label.set_halign(gtk4::Align::Start);
        label.set_css_classes(&["section-title"]);
        let entry = Entry::new();
        entry.set_hexpand(true);
        entry.set_placeholder_text(Some("Enter game name"));
        if let (Some(path), Some(mode)) = (self.pending_game_path.as_ref(), self.pending_add_mode) {
            if let Some(default_name) = Self::default_game_name_for_path(mode, path) {
                entry.set_text(&default_name);
            }
        }
        content.append(&label);
        content.append(&entry);

        let sender_clone = sender.clone();
        let handled = Rc::new(Cell::new(false));
        let handled_clone = handled.clone();
        dialog.connect_response(move |dialog, response| {
            if handled_clone.replace(true) {
                return;
            }
            if response == ResponseType::Accept {
                let name = entry.text().to_string();
                sender_clone.input(MainWindowMsg::GameNameConfirmed(name));
            } else {
                sender_clone.input(MainWindowMsg::AddGameCancelled);
            }

            dialog.close();
        });

        dialog.show();
        self.name_dialog = Some(dialog);
    }

    fn open_existing_game_location_dialog(
        &mut self,
        sender: ComponentSender<Self>,
        game_name: String,
    ) {
        if self.existing_location_dialog.is_some() {
            return;
        }

        let dialog = Dialog::builder()
            .title("Game Folder")
            .modal(true)
            .transient_for(&self.root_window)
            .build();
        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("Continue", ResponseType::Accept);

        let content = dialog.content_area();
        let layout = Box::new(Orientation::Vertical, 8);
        layout.set_margin_all(12);

        let title = Label::new(Some("Choose where to copy the game files"));
        title.set_halign(gtk4::Align::Start);
        title.set_css_classes(&["section-title"]);

        let hint = Label::new(Some(
            "Path is relative to the prefix 'games' folder.",
        ));
        hint.set_halign(gtk4::Align::Start);
        hint.set_wrap(true);
        hint.set_css_classes(&["muted"]);

        let location_label = Label::new(Some("Game folder (inside prefix/games)"));
        location_label.set_halign(gtk4::Align::Start);
        let location_entry = Entry::new();
        location_entry.set_placeholder_text(Some("e.g., MyGame"));
        location_entry.set_text(&game_name);

        layout.append(&title);
        layout.append(&hint);
        layout.append(&location_label);
        layout.append(&location_entry);
        content.append(&layout);

        let sender_clone = sender.clone();
        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                sender_clone.input(MainWindowMsg::ExistingGameLocationConfirmed(
                    location_entry.text().to_string(),
                ));
            } else {
                sender_clone.input(MainWindowMsg::ExistingGameLocationCancelled);
            }
            dialog.close();
        });

        dialog.show();
        self.existing_location_dialog = Some(dialog);
    }

    fn open_existing_source_folder_dialog(&mut self, sender: ComponentSender<Self>) {
        if self.game_path_dialog.is_some() {
            return;
        }

        let dialog = FileChooserNative::builder()
            .title("Select Game Folder to Copy")
            .action(FileChooserAction::SelectFolder)
            .accept_label("Select")
            .cancel_label("Cancel")
            .transient_for(&self.root_window)
            .build();

        let sender_clone = sender.clone();
        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                if let Some(file) = dialog.file() {
                    if let Some(path) = file.path() {
                        sender_clone.input(MainWindowMsg::ExistingSourceFolderSelected(path));
                    } else {
                        sender_clone.input(MainWindowMsg::ExistingSourceFolderCancelled);
                    }
                } else {
                    sender_clone.input(MainWindowMsg::ExistingSourceFolderCancelled);
                }
            } else {
                sender_clone.input(MainWindowMsg::ExistingSourceFolderCancelled);
            }
            dialog.destroy();
        });

        dialog.show();
        self.game_path_dialog = Some(dialog);
    }

    fn start_umu_db_sync(sender: ComponentSender<Self>) {
        thread::spawn(move || match UmuDatabase::load_or_fetch() {
            Ok(entries) => sender.input(MainWindowMsg::UmuDatabaseLoaded(entries)),
            Err(e) => sender.input(MainWindowMsg::UmuDatabaseFailed(e.to_string())),
        });
    }

    fn open_umu_match_dialog(
        &mut self,
        sender: ComponentSender<Self>,
        game_name: String,
        matches: Vec<UmuMatch>,
    ) {
        if self.umu_match_dialog.is_some() {
            return;
        }

        let dialog = Dialog::builder()
            .title("Match UMU Game")
            .modal(true)
            .transient_for(&self.root_window)
            .build();
        dialog.add_button("Skip", ResponseType::Cancel);
        dialog.add_button("Use Selection", ResponseType::Accept);

        let content = dialog.content_area();
        let layout = Box::new(Orientation::Vertical, 8);
        layout.set_margin_all(12);

        let title = Label::new(Some(&format!(
            "Select the UMU match for \"{}\"",
            game_name
        )));
        title.set_halign(gtk4::Align::Start);
        title.set_wrap(true);
        title.set_css_classes(&["section-title"]);

        let hint = Label::new(Some(
            "Pick the correct storefront entry. If none match, click Skip.",
        ));
        hint.set_halign(gtk4::Align::Start);
        hint.set_wrap(true);
        hint.set_css_classes(&["muted"]);

        let listbox = ListBox::new();
        listbox.set_selection_mode(SelectionMode::Single);

        for candidate in &matches {
            let entry = &candidate.entry;
            let row = ListBoxRow::new();
            let row_box = Box::new(Orientation::Vertical, 4);
            row_box.set_margin_all(8);

            let title_text = entry.title.as_deref().unwrap_or("Unknown title");
            let title_label = Label::new(Some(title_text));
            title_label.set_halign(gtk4::Align::Start);
            title_label.set_wrap(true);
            title_label.set_css_classes(&["card-title"]);

            let umu_id = entry.umu_id.as_deref().unwrap_or("unknown");
            let store = entry.store.as_deref().unwrap_or("unknown");
            let codename = entry.codename.as_deref().unwrap_or("unknown");
            let detail_text = format!("UMU ID: {umu_id} • Store: {store} • Codename: {codename}");
            let detail_label = Label::new(Some(&detail_text));
            detail_label.set_halign(gtk4::Align::Start);
            detail_label.set_wrap(true);
            detail_label.set_css_classes(&["muted"]);

            row_box.append(&title_label);
            row_box.append(&detail_label);

            if let Some(notes) = entry.notes.as_deref() {
                if !notes.trim().is_empty() {
                    let notes_label = Label::new(Some(notes));
                    notes_label.set_halign(gtk4::Align::Start);
                    notes_label.set_wrap(true);
                    notes_label.set_css_classes(&["muted"]);
                    row_box.append(&notes_label);
                }
            }

            row.set_child(Some(&row_box));
            listbox.append(&row);
        }

        if let Some(first_row) = listbox.row_at_index(0) {
            listbox.select_row(Some(&first_row));
        }

        let scroller = ScrolledWindow::new();
        scroller.set_vexpand(true);
        scroller.set_child(Some(&listbox));

        layout.append(&title);
        layout.append(&hint);
        layout.append(&scroller);
        content.append(&layout);

        let sender_clone = sender.clone();
        let matches_clone = matches.clone();
        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                if let Some(row) = listbox.selected_row() {
                    let index = row.index();
                    if index >= 0 {
                        if let Some(selected) = matches_clone.get(index as usize) {
                            sender_clone.input(MainWindowMsg::UmuMatchChosen {
                                game_id: selected.entry.umu_id.clone(),
                                store: selected.entry.store.clone(),
                            });
                        } else {
                            sender_clone.input(MainWindowMsg::UmuMatchChosen {
                                game_id: None,
                                store: None,
                            });
                        }
                    } else {
                        sender_clone.input(MainWindowMsg::UmuMatchChosen {
                            game_id: None,
                            store: None,
                        });
                    }
                } else {
                    sender_clone.input(MainWindowMsg::UmuMatchChosen {
                        game_id: None,
                        store: None,
                    });
                }
            } else {
                sender_clone.input(MainWindowMsg::UmuMatchChosen {
                    game_id: None,
                    store: None,
                });
            }

            sender_clone.input(MainWindowMsg::UmuMatchDialogClosed);
            dialog.close();
        });

        dialog.show();
        self.umu_match_dialog = Some(dialog);
    }

    fn open_dependency_dialog(
        &mut self,
        sender: ComponentSender<Self>,
        capsule_dir: PathBuf,
        metadata: CapsuleMetadata,
    ) {
        if self.dependency_dialog.is_some() {
            return;
        }

        let vcredist_cached = Self::vcredist_cache_path().is_file();
        let dxweb_cached = Self::dxweb_cache_path().is_file();

        let dialog = Dialog::builder()
            .title("Install Dependencies")
            .modal(true)
            .transient_for(&self.root_window)
            .build();
        dialog.add_button("Skip", ResponseType::Cancel);
        dialog.add_button("Install", ResponseType::Accept);

        let content = dialog.content_area();
        let layout = Box::new(Orientation::Vertical, 8);
        layout.set_margin_all(12);

        let title = Label::new(Some("Install optional dependencies?"));
        title.set_halign(gtk4::Align::Start);
        title.set_wrap(true);
        title.set_css_classes(&["section-title"]);

        let hint = Label::new(Some(
            "These installers are cached by linuxboy-setup.sh. Disable any you don't want.",
        ));
        hint.set_halign(gtk4::Align::Start);
        hint.set_wrap(true);
        hint.set_css_classes(&["muted"]);

        let vcredist_row = Box::new(Orientation::Vertical, 4);
        let vcredist_check = CheckButton::with_label("VC++ Redistributables (AIO)");
        vcredist_check.set_active(metadata.install_vcredist && vcredist_cached);
        vcredist_check.set_sensitive(vcredist_cached);
        let vcredist_status = Label::new(Some(if vcredist_cached {
            "Cached"
        } else {
            "Not downloaded (run setup script)"
        }));
        vcredist_status.set_halign(gtk4::Align::Start);
        vcredist_status.set_css_classes(&["muted"]);
        vcredist_row.append(&vcredist_check);
        vcredist_row.append(&vcredist_status);

        let dxweb_row = Box::new(Orientation::Vertical, 4);
        let dxweb_check = CheckButton::with_label("DirectX (June 2010) Redist");
        dxweb_check.set_active(metadata.install_dxweb && dxweb_cached);
        dxweb_check.set_sensitive(dxweb_cached);
        let dxweb_status = Label::new(Some(if dxweb_cached {
            "Cached"
        } else {
            "Not downloaded (run setup script)"
        }));
        dxweb_status.set_halign(gtk4::Align::Start);
        dxweb_status.set_css_classes(&["muted"]);
        dxweb_row.append(&dxweb_check);
        dxweb_row.append(&dxweb_status);

        layout.append(&title);
        layout.append(&hint);
        layout.append(&vcredist_row);
        layout.append(&dxweb_row);
        content.append(&layout);

        let sender_clone = sender.clone();
        let capsule_dir_clone = capsule_dir.clone();
        let vcredist_check_clone = vcredist_check.clone();
        let dxweb_check_clone = dxweb_check.clone();
        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                sender_clone.input(MainWindowMsg::DependenciesSelected {
                    capsule_dir: capsule_dir_clone.clone(),
                    install_vcredist: vcredist_check_clone.is_active(),
                    install_dxweb: dxweb_check_clone.is_active(),
                    force: false,
                });
            }
            sender_clone.input(MainWindowMsg::DependenciesDialogClosed);
            dialog.close();
        });

        dialog.show();
        self.dependency_dialog = Some(dialog);
    }

    fn start_dependency_install(
        &mut self,
        sender: ComponentSender<Self>,
        capsule_dir: PathBuf,
        metadata: CapsuleMetadata,
        install_vcredist: bool,
        install_dxweb: bool,
        force: bool,
    ) {
        if !Self::has_command("umu-run") {
            eprintln!("umu-run not found in PATH");
            return;
        }

        let proton_path = match self.runtime_mgr.latest_installed() {
            Ok(Some(path)) => path,
            Ok(None) => {
                eprintln!("No Proton-GE runtime installed");
                return;
            }
            Err(e) => {
                eprintln!("Failed to resolve Proton-GE runtime: {}", e);
                return;
            }
        };

        let home_path = capsule_dir.join(format!("{}.AppImage.home", metadata.name));
        let prefix_path = home_path.join("prefix");

        let mut tasks: Vec<(&'static str, PathBuf)> = Vec::new();
        if install_vcredist && (force || !Self::is_dependency_installed(&metadata, Self::DEP_VCREDIST))
        {
            let path = Self::vcredist_cache_path();
            if path.is_file() {
                tasks.push((Self::DEP_VCREDIST, path));
            } else {
                eprintln!("VC++ installer not cached; run linuxboy-setup.sh");
            }
        }

        if install_dxweb && (force || !Self::is_dependency_installed(&metadata, Self::DEP_DXWEB)) {
            let path = Self::dxweb_cache_path();
            if path.is_file() {
                tasks.push((Self::DEP_DXWEB, path));
            } else {
                eprintln!("DirectX redist not cached; run linuxboy-setup.sh");
            }
        }

        if tasks.is_empty() {
            return;
        }

        self.dependency_installs.insert(capsule_dir.clone());
        self.rebuild_games_list(sender.clone());

        let sender_clone = sender.clone();
        thread::spawn(move || {
            let mut installed: Vec<String> = Vec::new();
            for (dep, path) in tasks {
                let success = if dep == Self::DEP_DXWEB {
                    Self::install_directx_redist(&prefix_path, &proton_path, &metadata, &path)
                } else {
                    let mut cmd = Self::umu_base_command(&prefix_path, &proton_path, &metadata);
                    cmd.env("PROTON_USE_XALIA", "0");
                    cmd.arg(&path);
                    match cmd.status() {
                        Ok(status) => status.success(),
                        Err(e) => {
                            eprintln!("Failed to run dependency installer {:?}: {}", path, e);
                            false
                        }
                    }
                };

                if success {
                    installed.push(dep.to_string());
                } else {
                    eprintln!("Dependency installer failed: {:?}", path);
                }
            }

            let _ = sender_clone.input(MainWindowMsg::DependenciesFinished {
                capsule_dir,
                installed,
            });
        });
    }

    fn start_game(
        &mut self,
        sender: ComponentSender<Self>,
        capsule_dir: PathBuf,
    ) {
        let capsule = match Capsule::load_from_dir(&capsule_dir) {
            Ok(capsule) => capsule,
            Err(e) => {
                eprintln!("Failed to load capsule: {}", e);
                return;
            }
        };

        if capsule.metadata.executables.main.path.trim().is_empty() {
            eprintln!("No executable configured for {}", capsule.name);
            return;
        }

        if !Self::has_command("umu-run") {
            eprintln!("umu-run not found in PATH");
            return;
        }

        let proton_path = match self.runtime_mgr.latest_installed() {
            Ok(Some(path)) => path,
            Ok(None) => {
                eprintln!("No Proton-GE runtime installed");
                return;
            }
            Err(e) => {
                eprintln!("Failed to resolve Proton-GE runtime: {}", e);
                return;
            }
        };

        let home_path = capsule.capsule_dir.join(format!("{}.AppImage.home", capsule.name));
        let prefix_path = home_path.join("prefix");

        if !Self::run_umu_preflight(&prefix_path, &proton_path, &capsule.metadata) {
            eprintln!("UMU runtime preload failed.");
            return;
        }

        let exe_path = PathBuf::from(&capsule.metadata.executables.main.path);
        let mut cmd = Self::umu_base_command(&prefix_path, &proton_path, &capsule.metadata);
        cmd.arg(&exe_path);
        if let Some(exe_dir) = exe_path.parent().filter(|dir| dir.is_dir()) {
            cmd.current_dir(exe_dir);
        }

        let args = capsule.metadata.executables.main.args.trim();
        if !args.is_empty() {
            cmd.args(args.split_whitespace());
        }

        for trick in &capsule.metadata.protonfixes_tricks {
            cmd.arg(format!("-pf_tricks={}", trick));
        }
        for replace in &capsule.metadata.protonfixes_replace_cmds {
            cmd.arg(format!("-pf_replace_cmd={}", replace));
        }
        for option in &capsule.metadata.protonfixes_dxvk_sets {
            cmd.arg(format!("-pf_dxvk_set={}", option));
        }

        unsafe {
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }

        let sender_clone = sender.clone();
        thread::spawn(move || {
            let mut child = match cmd.spawn() {
                Ok(child) => child,
                Err(e) => {
                    eprintln!("Failed to launch game: {}", e);
                    let _ = sender_clone.input(MainWindowMsg::GameFinished {
                        capsule_dir,
                        success: false,
                    });
                    return;
                }
            };

            let pid = child.id() as i32;
            let pgid = unsafe { libc::getpgid(pid) };
            if pgid > 0 {
                let _ = sender_clone.input(MainWindowMsg::GameStarted {
                    capsule_dir: capsule_dir.clone(),
                    pgid,
                });
            }

            let success = child.wait().map(|status| status.success()).unwrap_or(false);
            let _ = sender_clone.input(MainWindowMsg::GameFinished {
                capsule_dir,
                success,
            });
        });
    }

    fn finalize_pending_game(
        &mut self,
        sender: ComponentSender<Self>,
        game_id: Option<String>,
        store: Option<String>,
    ) {
        self.pending_add_mode = None;
        self.pending_game_id = None;
        self.pending_store = None;
        let installer_path = match self.pending_game_path.take() {
            Some(path) => path,
            None => {
                eprintln!("No installer path selected");
                self.pending_game_name = None;
                return;
            }
        };

        let name = match self.pending_game_name.take() {
            Some(name) => name,
            None => {
                eprintln!("No pending game name available");
                return;
            }
        };

        if let Err(e) = fs::create_dir_all(&self.games_dir) {
            eprintln!("Failed to create games directory: {}", e);
            return;
        }

        let capsule_dir = self.unique_game_dir(&name);
        if let Err(e) = fs::create_dir_all(&capsule_dir) {
            eprintln!("Failed to create capsule directory: {}", e);
            return;
        }

        let mut metadata = CapsuleMetadata::default();
        metadata.name = name.clone();
        metadata.installer_path = Some(installer_path.to_string_lossy().to_string());
        metadata.install_state = InstallState::Installing;
        metadata.game_id = game_id;
        metadata.store = store;
        let home_path = capsule_dir.join(format!("{}.AppImage.home", name));
        let prefix_path = home_path.join("prefix");
        let default_game_dir = prefix_path.join("games").join(&metadata.name);
        metadata.game_dir = Some(default_game_dir.to_string_lossy().to_string());

        self.start_installer(&sender, capsule_dir, metadata, installer_path);
        sender.input(MainWindowMsg::LoadCapsules);
    }

    fn finalize_existing_game(
        &mut self,
        sender: ComponentSender<Self>,
        target_input: String,
    ) {
        self.pending_add_mode = None;
        let exe_path = match self.pending_game_path.take() {
            Some(path) => path,
            None => {
                eprintln!("No game executable selected");
                self.pending_game_name = None;
                self.pending_game_id = None;
                self.pending_store = None;
                return;
            }
        };

        let name = match self.pending_game_name.take() {
            Some(name) => name,
            None => {
                eprintln!("No pending game name available");
                self.pending_game_id = None;
                self.pending_store = None;
                return;
            }
        };

        let source_dir = match self.pending_source_folder.take() {
            Some(path) => path,
            None => {
                eprintln!("No source folder selected");
                self.pending_game_id = None;
                self.pending_store = None;
                return;
            }
        };

        let game_id = self.pending_game_id.take();
        let store = self.pending_store.take();

        if let Err(e) = fs::create_dir_all(&self.games_dir) {
            eprintln!("Failed to create games directory: {}", e);
            return;
        }

        let capsule_dir = self.unique_game_dir(&name);
        if let Err(e) = fs::create_dir_all(&capsule_dir) {
            eprintln!("Failed to create capsule directory: {}", e);
            return;
        }

        let home_path = capsule_dir.join(format!("{}.AppImage.home", name));
        let prefix_path = home_path.join("prefix");
        let games_root = prefix_path.join("games");
        if let Err(e) = fs::create_dir_all(prefix_path.join("drive_c")) {
            eprintln!("Failed to create prefix: {}", e);
            return;
        }
        if let Err(e) = fs::create_dir_all(&games_root) {
            eprintln!("Failed to create games folder: {}", e);
            return;
        }

        let relative_folder = Self::resolve_relative_game_folder(&name, &target_input);
        let mut dest_dir = games_root.join(relative_folder);
        dest_dir = Self::unique_path(dest_dir);

        let mut should_copy = true;
        if let (Ok(src), Ok(dest)) = (fs::canonicalize(&source_dir), fs::canonicalize(&dest_dir)) {
            if src == dest {
                should_copy = false;
            }
        }

        if exe_path.strip_prefix(&source_dir).is_err() {
            eprintln!("Selected executable is not inside the chosen folder.");
            return;
        }

        if should_copy {
            if let Err(e) = Self::copy_dir_recursive(&source_dir, &dest_dir) {
                eprintln!("Failed to copy game files: {}", e);
                return;
            }
        }

        let relative_exe = exe_path
            .strip_prefix(&source_dir)
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                exe_path
                    .file_name()
                    .map(PathBuf::from)
                    .unwrap_or_else(PathBuf::new)
            });
        let new_exe_path = dest_dir.join(relative_exe);

        let mut metadata = CapsuleMetadata::default();
        metadata.name = name.clone();
        metadata.install_state = InstallState::Installed;
        metadata.executables.main.path = new_exe_path.to_string_lossy().to_string();
        metadata.game_id = game_id;
        metadata.store = store;
        metadata.game_dir = Some(dest_dir.to_string_lossy().to_string());

        let capsule = Capsule {
            name: metadata.name.clone(),
            capsule_dir: capsule_dir.clone(),
            home_path,
            metadata: metadata.clone(),
        };

        if let Err(e) = capsule.save_metadata() {
            eprintln!("Failed to save metadata: {}", e);
            return;
        }

        if self.should_prompt_dependencies(&metadata) {
            self.open_dependency_dialog(sender.clone(), capsule_dir.clone(), metadata);
        }
        sender.input(MainWindowMsg::LoadCapsules);
    }

    fn find_umu_matches(&self, title: &str) -> Vec<UmuMatch> {
        if !self.umu_loaded || self.umu_entries.is_empty() {
            return Vec::new();
        }

        let normalized_input = UmuDatabase::normalize_title(title);
        if normalized_input.is_empty() {
            return Vec::new();
        }

        let mut matches: Vec<UmuMatch> = Vec::new();
        for entry in &self.umu_entries {
            let mut best_score: Option<i32> = None;
            if let Some(entry_title) = entry.title.as_deref() {
                if let Some(score) = Self::score_match(title, entry_title) {
                    best_score = Some(score);
                }
            }
            if let Some(acronym) = entry.acronym.as_deref() {
                if let Some(score) = Self::score_acronym(title, acronym) {
                    best_score = Some(best_score.map_or(score, |current| current.min(score)));
                }
            }
            if let Some(score) = best_score {
                matches.push(UmuMatch {
                    entry: entry.clone(),
                    score,
                });
            }
        }

        if matches.is_empty() {
            return matches;
        }

        let mut seen = HashSet::new();
        matches.retain(|candidate| {
            let key = (
                candidate.entry.umu_id.clone().unwrap_or_default(),
                candidate.entry.store.clone().unwrap_or_default(),
                candidate.entry.codename.clone().unwrap_or_default(),
            );
            seen.insert(key)
        });

        matches.sort_by(|a, b| {
            a.score
                .cmp(&b.score)
                .then_with(|| {
                    let a_title = a.entry.title.as_deref().unwrap_or("");
                    let b_title = b.entry.title.as_deref().unwrap_or("");
                    a_title.len().cmp(&b_title.len())
                })
                .then_with(|| {
                    let a_title = a.entry.title.as_deref().unwrap_or("");
                    let b_title = b.entry.title.as_deref().unwrap_or("");
                    a_title.cmp(b_title)
                })
                .then_with(|| {
                    let a_store = a.entry.store.as_deref().unwrap_or("");
                    let b_store = b.entry.store.as_deref().unwrap_or("");
                    a_store.cmp(b_store)
                })
        });

        matches.truncate(20);
        matches
    }

    fn open_game_settings_dialog(&mut self, sender: ComponentSender<Self>, capsule_dir: PathBuf) {
        if self.settings_dialog.is_some() {
            return;
        }

        let capsule = match Capsule::load_from_dir(&capsule_dir) {
            Ok(capsule) => capsule,
            Err(e) => {
                eprintln!("Failed to load capsule: {}", e);
                return;
            }
        };

        let dialog = Dialog::builder()
            .title("Game Settings")
            .modal(true)
            .transient_for(&self.root_window)
            .build();
        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("Save", ResponseType::Accept);

        let content = dialog.content_area();
        let layout = Box::new(Orientation::Vertical, 8);
        layout.set_margin_all(12);

        let exe_label = Label::new(Some("Executable"));
        exe_label.set_halign(gtk4::Align::Start);

        let exe_row = Box::new(Orientation::Horizontal, 8);
        exe_row.set_hexpand(true);

        let exe_entry = Entry::new();
        exe_entry.set_hexpand(true);
        exe_entry.set_placeholder_text(Some("Path to game executable (.exe)"));
        if !capsule.metadata.executables.main.path.trim().is_empty() {
            exe_entry.set_text(&capsule.metadata.executables.main.path);
        }

        let exe_entry_clone = exe_entry.clone();
        let root_window = self.root_window.clone();
        let browse_button = Button::with_label("Browse");
        browse_button.connect_clicked(move |_| {
            let dialog = FileChooserNative::builder()
                .title("Select Game Executable")
                .action(FileChooserAction::Open)
                .accept_label("Select")
                .cancel_label("Cancel")
                .transient_for(&root_window)
                .build();

            let filter = FileFilter::new();
            filter.add_suffix("exe");
            filter.set_name(Some("Windows executables (.exe)"));
            dialog.add_filter(&filter);

            let exe_entry_inner = exe_entry_clone.clone();
            dialog.connect_response(move |dialog, response| {
                if response == ResponseType::Accept {
                    if let Some(file) = dialog.file() {
                        if let Some(path) = file.path() {
                            exe_entry_inner.set_text(&path.to_string_lossy());
                        }
                    }
                }
                dialog.destroy();
            });

            dialog.show();
        });

        exe_row.append(&exe_entry);
        exe_row.append(&browse_button);

        let game_id_label = Label::new(Some("UMU Game ID (optional)"));
        game_id_label.set_halign(gtk4::Align::Start);
        let game_id_entry = Entry::new();
        game_id_entry.set_placeholder_text(Some("e.g., umu-starcitizen"));
        if let Some(game_id) = &capsule.metadata.game_id {
            game_id_entry.set_text(game_id);
        }

        let store_label = Label::new(Some("Store (optional)"));
        store_label.set_halign(gtk4::Align::Start);
        let store_entry = Entry::new();
        store_entry.set_placeholder_text(Some("e.g., steam, gog, egs, none"));
        if let Some(store) = &capsule.metadata.store {
            store_entry.set_text(store);
        }

        let deps_title = Label::new(Some("Dependencies"));
        deps_title.set_halign(gtk4::Align::Start);
        deps_title.set_css_classes(&["section-title"]);
        let deps_hint = Label::new(Some(
            "Requires cached installers from linuxboy-setup.sh.",
        ));
        deps_hint.set_halign(gtk4::Align::Start);
        deps_hint.set_wrap(true);
        deps_hint.set_css_classes(&["muted"]);

        let vcredist_check = CheckButton::with_label("Install VC++ Redistributables (AIO)");
        vcredist_check.set_active(capsule.metadata.install_vcredist);
        let dxweb_check = CheckButton::with_label("Install DirectX (June 2010) Redist");
        dxweb_check.set_active(capsule.metadata.install_dxweb);

        let install_deps_button = Button::with_label("Install dependencies now");
        install_deps_button.add_css_class("suggested-action");

        let input_title = Label::new(Some("Input & UI"));
        input_title.set_halign(gtk4::Align::Start);
        input_title.set_css_classes(&["section-title"]);

        let xalia_check = CheckButton::with_label("Enable Xalia controller UI layer (may disable mouse)");
        xalia_check.set_active(capsule.metadata.xalia_enabled);

        let pf_title = Label::new(Some("Protonfixes Overrides"));
        pf_title.set_halign(gtk4::Align::Start);
        pf_title.set_css_classes(&["section-title"]);

        let pf_disable = CheckButton::with_label("Disable Protonfixes for this game");
        pf_disable.set_active(capsule.metadata.protonfixes_disable);

        let pf_tricks_label = Label::new(Some("Winetricks / Protontricks verbs"));
        pf_tricks_label.set_halign(gtk4::Align::Start);
        let pf_tricks_entry = Entry::new();
        pf_tricks_entry.set_placeholder_text(Some("xliveless d3dcompiler_47"));
        if !capsule.metadata.protonfixes_tricks.is_empty() {
            pf_tricks_entry.set_text(&capsule.metadata.protonfixes_tricks.join(" "));
        }

        let pf_replace_label = Label::new(Some("Command replacements"));
        pf_replace_label.set_halign(gtk4::Align::Start);
        let pf_replace_entry = Entry::new();
        pf_replace_entry.set_placeholder_text(Some("/launcher.exe=/game.exe"));
        if !capsule.metadata.protonfixes_replace_cmds.is_empty() {
            pf_replace_entry.set_text(&capsule.metadata.protonfixes_replace_cmds.join(" "));
        }

        let pf_dxvk_label = Label::new(Some("DXVK options"));
        pf_dxvk_label.set_halign(gtk4::Align::Start);
        let pf_dxvk_entry = Entry::new();
        pf_dxvk_entry.set_placeholder_text(Some("dxgi.maxFrameRate=60"));
        if !capsule.metadata.protonfixes_dxvk_sets.is_empty() {
            pf_dxvk_entry.set_text(&capsule.metadata.protonfixes_dxvk_sets.join(" "));
        }

        layout.append(&exe_label);
        layout.append(&exe_row);
        layout.append(&game_id_label);
        layout.append(&game_id_entry);
        layout.append(&store_label);
        layout.append(&store_entry);
        layout.append(&deps_title);
        layout.append(&deps_hint);
        layout.append(&vcredist_check);
        layout.append(&dxweb_check);
        layout.append(&install_deps_button);
        layout.append(&input_title);
        layout.append(&xalia_check);
        layout.append(&pf_title);
        layout.append(&pf_disable);
        layout.append(&pf_tricks_label);
        layout.append(&pf_tricks_entry);
        layout.append(&pf_replace_label);
        layout.append(&pf_replace_entry);
        layout.append(&pf_dxvk_label);
        layout.append(&pf_dxvk_entry);
        content.append(&layout);

        let sender_clone = sender.clone();
        let capsule_dir_clone = capsule_dir.clone();
        let exe_entry_clone = exe_entry.clone();
        let game_id_entry_clone = game_id_entry.clone();
        let store_entry_clone = store_entry.clone();
        let vcredist_check_clone = vcredist_check.clone();
        let dxweb_check_clone = dxweb_check.clone();
        let xalia_check_clone = xalia_check.clone();
        let pf_disable_clone = pf_disable.clone();
        let pf_tricks_entry_clone = pf_tricks_entry.clone();
        let pf_replace_entry_clone = pf_replace_entry.clone();
        let pf_dxvk_entry_clone = pf_dxvk_entry.clone();
        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                let exe_path = exe_entry_clone.text().to_string();
                let game_id_text = game_id_entry_clone.text().trim().to_string();
                let store_text = store_entry_clone.text().trim().to_string();
                let install_vcredist = vcredist_check_clone.is_active();
                let install_dxweb = dxweb_check_clone.is_active();
                let protonfixes_disable = pf_disable_clone.is_active();
                let xalia_enabled = xalia_check_clone.is_active();
                let protonfixes_tricks = MainWindow::parse_list_input(&pf_tricks_entry_clone.text());
                let protonfixes_replace_cmds =
                    MainWindow::parse_list_input(&pf_replace_entry_clone.text());
                let protonfixes_dxvk_sets = MainWindow::parse_list_input(&pf_dxvk_entry_clone.text());
                let game_id = if game_id_text.is_empty() {
                    None
                } else {
                    Some(game_id_text)
                };
                let store = if store_text.is_empty() {
                    None
                } else {
                    Some(store_text)
                };
                sender_clone.input(MainWindowMsg::SaveGameSettings {
                    capsule_dir: capsule_dir_clone.clone(),
                    exe_path,
                    game_id,
                    store,
                    install_vcredist,
                    install_dxweb,
                    protonfixes_disable,
                    xalia_enabled,
                    protonfixes_tricks,
                    protonfixes_replace_cmds,
                    protonfixes_dxvk_sets,
                });
            }

            sender_clone.input(MainWindowMsg::SettingsDialogClosed);
            dialog.close();
        });

        let sender_clone = sender.clone();
        let capsule_dir_clone = capsule_dir.clone();
        let exe_entry_clone = exe_entry.clone();
        let game_id_entry_clone = game_id_entry.clone();
        let store_entry_clone = store_entry.clone();
        let vcredist_check_clone = vcredist_check.clone();
        let dxweb_check_clone = dxweb_check.clone();
        let xalia_check_clone = xalia_check.clone();
        let pf_disable_clone = pf_disable.clone();
        let pf_tricks_entry_clone = pf_tricks_entry.clone();
        let pf_replace_entry_clone = pf_replace_entry.clone();
        let pf_dxvk_entry_clone = pf_dxvk_entry.clone();
        let dialog_clone = dialog.clone();
        install_deps_button.connect_clicked(move |_| {
            let exe_path = exe_entry_clone.text().to_string();
            let game_id_text = game_id_entry_clone.text().trim().to_string();
            let store_text = store_entry_clone.text().trim().to_string();
            let install_vcredist = vcredist_check_clone.is_active();
            let install_dxweb = dxweb_check_clone.is_active();
            let protonfixes_disable = pf_disable_clone.is_active();
            let xalia_enabled = xalia_check_clone.is_active();
            let protonfixes_tricks = MainWindow::parse_list_input(&pf_tricks_entry_clone.text());
            let protonfixes_replace_cmds =
                MainWindow::parse_list_input(&pf_replace_entry_clone.text());
            let protonfixes_dxvk_sets = MainWindow::parse_list_input(&pf_dxvk_entry_clone.text());
            let game_id = if game_id_text.is_empty() {
                None
            } else {
                Some(game_id_text)
            };
            let store = if store_text.is_empty() {
                None
            } else {
                Some(store_text)
            };
            sender_clone.input(MainWindowMsg::SaveGameSettings {
                capsule_dir: capsule_dir_clone.clone(),
                exe_path,
                game_id,
                store,
                install_vcredist,
                install_dxweb,
                protonfixes_disable,
                xalia_enabled,
                protonfixes_tricks,
                protonfixes_replace_cmds,
                protonfixes_dxvk_sets,
            });
            sender_clone.input(MainWindowMsg::DependenciesSelected {
                capsule_dir: capsule_dir_clone.clone(),
                install_vcredist,
                install_dxweb,
                force: true,
            });
            sender_clone.input(MainWindowMsg::SettingsDialogClosed);
            dialog_clone.close();
        });

        dialog.show();
        self.settings_dialog = Some(dialog);
    }

    fn start_installer(
        &mut self,
        sender: &ComponentSender<Self>,
        capsule_dir: PathBuf,
        mut metadata: CapsuleMetadata,
        installer_path: PathBuf,
    ) {
        if !Self::has_command("umu-run") {
            eprintln!("umu-run not found in PATH");
            return;
        }

        let proton_path = match self.runtime_mgr.latest_installed() {
            Ok(Some(path)) => path,
            Ok(None) => {
                eprintln!("No Proton-GE runtime installed");
                return;
            }
            Err(e) => {
                eprintln!("Failed to resolve Proton-GE runtime: {}", e);
                return;
            }
        };

        let home_path = capsule_dir.join(format!("{}.AppImage.home", metadata.name));
        let prefix_path = home_path.join("prefix");
        if let Err(e) = fs::create_dir_all(prefix_path.join("drive_c")) {
            eprintln!("Failed to create prefix: {}", e);
            return;
        }
        if let Err(e) = fs::create_dir_all(prefix_path.join("games")) {
            eprintln!("Failed to create games folder: {}", e);
            return;
        }
        if let Some(game_dir) = metadata.game_dir.as_deref() {
            let path = PathBuf::from(game_dir);
            if let Err(e) = fs::create_dir_all(&path) {
                eprintln!("Failed to create default game folder: {}", e);
                return;
            }
        }

        metadata.installer_path = Some(installer_path.to_string_lossy().to_string());
        metadata.install_state = InstallState::Installing;

        let capsule = Capsule {
            name: metadata.name.clone(),
            capsule_dir: capsule_dir.clone(),
            home_path,
            metadata: metadata.clone(),
        };

        if let Err(e) = capsule.save_metadata() {
            eprintln!("Failed to save metadata: {}", e);
            return;
        }

        self.preparing_installs.insert(capsule_dir.clone());
        self.rebuild_games_list(sender.clone());

        let env_metadata = metadata.clone();
        let sender_clone = sender.clone();
        thread::spawn(move || {
            println!("Preloading UMU runtime...");
            if !Self::run_umu_preflight(&prefix_path, &proton_path, &env_metadata) {
                eprintln!("UMU runtime preload failed.");
                let _ = sender_clone.input(MainWindowMsg::InstallerFinished {
                    capsule_dir,
                    success: false,
                });
                return;
            }

            let mut cmd = Self::umu_base_command(&prefix_path, &proton_path, &env_metadata);
            // Avoid Xalia UI automation errors during installers.
            cmd.env("PROTON_USE_XALIA", "0");
            cmd.arg(&installer_path);

            unsafe {
                cmd.pre_exec(|| {
                    libc::setpgid(0, 0);
                    Ok(())
                });
            }

            let mut child = match cmd.spawn() {
                Ok(child) => child,
                Err(e) => {
                    eprintln!("Failed to launch installer: {}", e);
                    let _ = sender_clone.input(MainWindowMsg::InstallerFinished {
                        capsule_dir,
                        success: false,
                    });
                    return;
                }
            };

            let pid = child.id() as i32;
            let pgid = unsafe { libc::getpgid(pid) };
            if pgid > 0 {
                let _ = sender_clone.input(MainWindowMsg::InstallerStarted {
                    capsule_dir: capsule_dir.clone(),
                    pgid,
                });
            }

            let success = child.wait().map(|status| status.success()).unwrap_or(false);
            let _ = sender_clone.input(MainWindowMsg::InstallerFinished {
                capsule_dir,
                success,
            });
        });
    }

    fn umu_base_command(
        prefix_path: &PathBuf,
        proton_path: &PathBuf,
        metadata: &CapsuleMetadata,
    ) -> Command {
        let mut cmd = Command::new("umu-run");
        cmd.env("WINEPREFIX", prefix_path);
        cmd.env("PROTONPATH", proton_path);
        let game_id = metadata
            .game_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("umu-default");
        let store = metadata
            .store
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("none");
        cmd.env("GAMEID", game_id);
        cmd.env("STORE", store);
        cmd.env("PROTON_USE_XALIA", if metadata.xalia_enabled { "1" } else { "0" });
        if metadata.protonfixes_disable {
            cmd.env("PROTONFIXES_DISABLE", "1");
        }
        for (key, value) in &metadata.env_vars {
            let trimmed = key.trim();
            if !trimmed.is_empty() {
                cmd.env(trimmed, value);
            }
        }
        cmd
    }

    fn install_directx_redist(
        prefix_path: &PathBuf,
        proton_path: &PathBuf,
        metadata: &CapsuleMetadata,
        redist_path: &Path,
    ) -> bool {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp_dir_name = format!("linuxboy-dxredist-{}", nanos);
        let host_temp_dir = prefix_path
            .join("drive_c")
            .join("linuxboy-temp")
            .join(&temp_dir_name);
        let windows_temp_dir = format!("C:\\\\linuxboy-temp\\\\{}", temp_dir_name);

        if let Err(e) = fs::create_dir_all(&host_temp_dir) {
            eprintln!("Failed to create DirectX temp dir: {}", e);
            return false;
        }

        let extract_arg = format!("/T:{}", windows_temp_dir);
        let mut extract_cmd = Self::umu_base_command(prefix_path, proton_path, metadata);
        extract_cmd.env("PROTON_USE_XALIA", "0");
        extract_cmd.arg(redist_path);
        extract_cmd.arg("/Q");
        extract_cmd.arg(extract_arg);
        extract_cmd.arg("/C");
        let extracted = match extract_cmd.status() {
            Ok(status) => status.success(),
            Err(e) => {
                eprintln!("Failed to extract DirectX redist: {}", e);
                false
            }
        };
        if !extracted {
            let _ = fs::remove_dir_all(&host_temp_dir);
            return false;
        }

        let dxsetup_path = host_temp_dir.join("DXSETUP.exe");
        if !dxsetup_path.is_file() {
            eprintln!("DirectX redist extraction missing DXSETUP.exe");
            let _ = fs::remove_dir_all(&host_temp_dir);
            return false;
        }

        let mut install_cmd = Self::umu_base_command(prefix_path, proton_path, metadata);
        install_cmd.env("PROTON_USE_XALIA", "0");
        install_cmd.arg(&dxsetup_path);
        install_cmd.arg("/silent");
        let success = match install_cmd.status() {
            Ok(status) => status.success(),
            Err(e) => {
                eprintln!("Failed to run DXSETUP.exe: {}", e);
                false
            }
        };
        let _ = fs::remove_dir_all(&host_temp_dir);
        success
    }

    fn run_umu_preflight(
        prefix_path: &PathBuf,
        proton_path: &PathBuf,
        metadata: &CapsuleMetadata,
    ) -> bool {
        let mut cmd = Self::umu_base_command(prefix_path, proton_path, metadata);
        // Avoid Xalia UI automation errors during preflight.
        cmd.env("PROTON_USE_XALIA", "0");
        // Run a harmless command to force prefix/runtime initialization.
        cmd.arg("cmd");
        cmd.arg("/c");
        cmd.arg("exit");
        match cmd.status() {
            Ok(status) => status.success(),
            Err(e) => {
                eprintln!("Failed to preload UMU runtime: {}", e);
                false
            }
        }
    }

    fn rebuild_games_list(&mut self, sender: ComponentSender<Self>) {
        let list = &self.games_list;
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }

        if self.capsules.is_empty() {
            let empty = Box::new(Orientation::Horizontal, 12);
            empty.set_margin_all(8);
            empty.set_css_classes(&["card"]);

            let icon = Image::from_icon_name("applications-games-symbolic");
            icon.set_pixel_size(28);
            icon.set_halign(gtk4::Align::Start);
            icon.set_valign(gtk4::Align::Start);

            let text = Box::new(Orientation::Vertical, 6);
            text.set_hexpand(true);

            let title = Label::new(Some("No games yet"));
            title.set_css_classes(&["card-title"]);
            title.set_halign(gtk4::Align::Start);

            let subtitle = Label::new(Some(
                "Add an installer to create your first portable capsule.",
            ));
            subtitle.set_css_classes(&["muted"]);
            subtitle.set_halign(gtk4::Align::Start);
            subtitle.set_wrap(true);

            text.append(&title);
            text.append(&subtitle);

            empty.append(&icon);
            empty.append(&text);
            list.append(&empty);
            return;
        }

        for capsule in &self.capsules {
            let card = Box::new(Orientation::Vertical, 8);
            card.set_margin_bottom(12);
            card.set_hexpand(true);
            card.set_css_classes(&["card"]);

            let header = Box::new(Orientation::Horizontal, 10);
            header.set_hexpand(true);

            let icon = Image::from_icon_name("applications-games-symbolic");
            icon.set_pixel_size(24);
            icon.set_halign(gtk4::Align::Start);

            let name = Label::new(Some(&capsule.name));
            name.set_halign(gtk4::Align::Start);
            name.set_hexpand(true);
            name.set_css_classes(&["card-title"]);

            let status_text = match capsule.metadata.install_state {
                InstallState::Installing => "Installing",
                InstallState::Installed => "Installed",
            };
            let status_class = match capsule.metadata.install_state {
                InstallState::Installing => "pill-warning",
                InstallState::Installed => "pill-installed",
            };
            let status = Label::new(Some(status_text));
            status.set_css_classes(&["pill", status_class]);

            let spacer = Box::new(Orientation::Horizontal, 0);
            spacer.set_hexpand(true);

            header.append(&icon);
            header.append(&name);
            header.append(&spacer);
            header.append(&status);

            let installing = capsule.metadata.install_state == InstallState::Installing;
            let is_running = self.active_installs.contains_key(&capsule.capsule_dir);
            let is_preparing = self.preparing_installs.contains(&capsule.capsule_dir);
            let deps_running = self.dependency_installs.contains(&capsule.capsule_dir);
            let game_running = self.active_games.contains_key(&capsule.capsule_dir);
            let exe_missing = capsule.metadata.executables.main.path.trim().is_empty();
            let detail_text = if deps_running {
                "Installing dependencies"
            } else if game_running {
                "Game running"
            } else if installing {
                if is_preparing {
                    "Preparing runtime"
                } else if is_running {
                    "Installer running"
                } else {
                    "Installer paused"
                }
            } else if exe_missing {
                "Select executable to finish setup"
            } else {
                "Ready to play"
            };

            let detail = Label::new(Some(detail_text));
            detail.set_css_classes(&["muted"]);
            detail.set_halign(gtk4::Align::Start);
            detail.set_margin_top(2);

            let actions = Box::new(Orientation::Horizontal, 8);
            actions.set_halign(gtk4::Align::Start);

            let edit_dir = capsule.capsule_dir.clone();
            let edit_sender = sender.clone();
            let edit_button = Button::with_label("Edit");
            edit_button.add_css_class("flat");
            edit_button.connect_clicked(move |_| {
                edit_sender.input(MainWindowMsg::EditGame(edit_dir.clone()));
            });
            actions.append(&edit_button);

            let delete_dir = capsule.capsule_dir.clone();
            let delete_sender = sender.clone();
            let delete_button = Button::with_label("Delete");
            delete_button.add_css_class("destructive-action");
            delete_button.connect_clicked(move |_| {
                delete_sender.input(MainWindowMsg::DeleteGame(delete_dir.clone()));
            });
            actions.append(&delete_button);

            if installing && is_running {
                let kill_dir = capsule.capsule_dir.clone();
                let kill_sender = sender.clone();
                let kill_button = Button::with_label("Kill installer");
                kill_button.add_css_class("destructive-action");
                kill_button.connect_clicked(move |_| {
                    kill_sender.input(MainWindowMsg::KillInstall(kill_dir.clone()));
                });
                actions.append(&kill_button);
            } else if installing && !is_preparing {
                let resume_dir = capsule.capsule_dir.clone();
                let resume_sender = sender.clone();
                let resume_button = Button::with_label("Resume setup");
                resume_button.add_css_class("suggested-action");
                resume_button.connect_clicked(move |_| {
                    resume_sender.input(MainWindowMsg::ResumeInstall(resume_dir.clone()));
                });
                actions.append(&resume_button);

                let finish_dir = capsule.capsule_dir.clone();
                let finish_sender = sender.clone();
                let finish_button = Button::with_label("Finish setup");
                finish_button.add_css_class("flat");
                finish_button.connect_clicked(move |_| {
                    finish_sender.input(MainWindowMsg::MarkInstallComplete(finish_dir.clone()));
                });
                actions.append(&finish_button);
            }

            if !installing && !exe_missing {
                let play_dir = capsule.capsule_dir.clone();
                let play_sender = sender.clone();
                let play_button = Button::with_label(if game_running { "Running" } else { "Play" });
                play_button.add_css_class("suggested-action");
                play_button.set_sensitive(!game_running);
                play_button.connect_clicked(move |_| {
                    play_sender.input(MainWindowMsg::LaunchGame(play_dir.clone()));
                });
                actions.append(&play_button);
            }

            card.append(&header);
            card.append(&detail);
            if let Some(store) = capsule
                .metadata
                .store
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let store_label = Label::new(Some(&format!("Store: {}", store)));
                store_label.set_css_classes(&["muted"]);
                store_label.set_halign(gtk4::Align::Start);
                card.append(&store_label);
            }
            card.append(&actions);
            list.append(&card);
        }
    }
}

#[allow(unused_assignments)]
#[relm4::component(pub)]
impl SimpleComponent for MainWindow {
    type Init = ();
    type Input = MainWindowMsg;
    type Output = ();

    view! {
        #[root]
        ApplicationWindow {
            set_title: Some("LinuxBoy"),
            set_default_width: 840,
            set_default_height: 720,

            #[wrap(Some)]
            set_child = &Box {
                set_orientation: Orientation::Vertical,
                set_spacing: 0,
                set_hexpand: true,
                set_vexpand: true,

                // Header bar
                append = &Box {
                    set_orientation: Orientation::Horizontal,
                    set_spacing: 12,
                    set_margin_start: 20,
                    set_margin_end: 20,
                    set_margin_top: 24,
                    set_margin_bottom: 14,
                    set_css_classes: &["topbar"],

                    append = &Image {
                        set_icon_name: Some("applications-games-symbolic"),
                        set_pixel_size: 28,
                    },

                    append = &Box {
                        set_orientation: Orientation::Vertical,
                        set_spacing: 0,

                        append = &Label {
                            set_label: "LinuxBoy",
                            set_css_classes: &["app-title"],
                            set_halign: gtk4::Align::Start,
                        },
                    },

                    append = &Box {
                        set_hexpand: true,
                    },

                    append = &Button {
                        set_css_classes: &["accent"],
                        #[wrap(Some)]
                        set_child = &Box {
                            set_orientation: Orientation::Horizontal,
                            set_spacing: 6,

                            append = &Image {
                                set_icon_name: Some("list-add-symbolic"),
                                set_pixel_size: 16,
                            },

                            append = &Label {
                                set_label: "Add Game",
                            },
                        },
                        connect_clicked => MainWindowMsg::OpenAddGame,
                    },
                },

                // Main content area
                append = &Box {
                    set_orientation: Orientation::Vertical,
                    set_hexpand: true,
                    set_vexpand: true,
                    set_margin_start: 12,
                    set_margin_end: 12,
                    set_margin_top: 10,

                    #[local_ref]
                    library_page -> Box {},
                },

                // Status bar
                append = &Box {
                    set_orientation: Orientation::Horizontal,
                    set_spacing: 12,
                    set_margin_start: 20,
                    set_margin_end: 20,
                    set_margin_top: 24,
                    set_margin_bottom: 28,
                    set_css_classes: &["status-bar"],

                    append = &Label {
                        #[watch]
                        set_label: &format!("{} games", model.capsules.len()),
                        set_css_classes: &["muted"],
                    },

                    append = &Box {
                        set_hexpand: true,
                    },

                    append = &Button {
                        #[watch]
                        set_label: &match model.system_check.status {
                            SystemStatus::AllInstalled => "System Ready",
                            SystemStatus::PartiallyInstalled => "Setup Incomplete",
                            SystemStatus::NothingInstalled => "Setup Required",
                        },
                        #[watch]
                        set_css_classes: &match model.system_check.status {
                            SystemStatus::AllInstalled => ["pill", "pill-installed"],
                            SystemStatus::PartiallyInstalled => ["pill", "pill-warning"],
                            SystemStatus::NothingInstalled => ["pill", "pill-missing"],
                        },
                        #[watch]
                        set_tooltip_text: Some(&model.system_check.status_message()),
                        set_halign: gtk4::Align::End,
                        connect_clicked => MainWindowMsg::OpenSystemSetup,
                    },
                },
            },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let games_dir = dirs::home_dir()
            .unwrap_or_default()
            .join("Games");

        // Check system on startup
        let system_check = SystemCheck::check();
        println!("System check: {:?}", system_check.status);

        let games_list = Box::new(Orientation::Vertical, 16);
        games_list.set_margin_all(0);
        games_list.set_valign(gtk4::Align::Start);
        games_list.set_hexpand(true);

        let library_count_label = Label::new(None);
        library_count_label.set_css_classes(&["muted"]);
        library_count_label.set_halign(gtk4::Align::Start);

        let library_page = Box::new(Orientation::Vertical, 16);
        library_page.set_margin_all(20);
        library_page.set_hexpand(true);
        library_page.set_vexpand(true);
        library_page.set_halign(gtk4::Align::Start);

        let library_header = Box::new(Orientation::Horizontal, 12);
        library_header.set_hexpand(true);
        library_header.set_halign(gtk4::Align::Start);

        let library_icon = Image::from_icon_name("folder-open-symbolic");
        library_icon.set_pixel_size(24);

        let library_title = Label::new(Some("Library"));
        library_title.set_css_classes(&["section-title"]);
        library_title.set_halign(gtk4::Align::Start);

        let library_spacer = Box::new(Orientation::Horizontal, 0);
        library_spacer.set_hexpand(true);

        library_header.append(&library_icon);
        library_header.append(&library_title);
        library_header.append(&library_spacer);
        library_header.append(&library_count_label);

        let library_body = Box::new(Orientation::Vertical, 0);
        library_body.set_halign(gtk4::Align::Start);
        library_body.set_hexpand(true);
        library_body.set_vexpand(true);
        library_body.set_width_request(840);

        let games_scroller = ScrolledWindow::new();
        games_scroller.set_hexpand(true);
        games_scroller.set_vexpand(true);
        games_scroller.set_child(Some(&games_list));
        library_body.append(&games_scroller);

        library_page.append(&library_header);
        library_page.append(&library_body);

        let model = MainWindow {
            capsules: Vec::new(),
            games_dir,
            system_check,
            system_setup_dialog: None,
            runtime_mgr: RuntimeManager::new(),
            add_game_dialog: None,
            game_path_dialog: None,
            name_dialog: None,
            settings_dialog: None,
            umu_match_dialog: None,
            dependency_dialog: None,
            existing_location_dialog: None,
            pending_add_mode: None,
            pending_game_path: None,
            pending_source_folder: None,
            pending_game_name: None,
            pending_game_id: None,
            pending_store: None,
            pending_settings_capsule: None,
            active_installs: HashMap::new(),
            active_games: HashMap::new(),
            preparing_installs: HashSet::new(),
            dependency_installs: HashSet::new(),
            umu_entries: Vec::new(),
            umu_loaded: false,
            umu_load_error: None,
            games_list: games_list.clone(),
            library_count_label,
            root_window: root.clone(),
        };

        model.update_library_labels();

        let widgets = view_output!();

        // Load capsules on startup
        sender.input(MainWindowMsg::LoadCapsules);
        Self::start_umu_db_sync(sender.clone());

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            MainWindowMsg::LoadCapsules => {
                match Capsule::scan_directory(&self.games_dir) {
                    Ok(capsules) => {
                        self.capsules = capsules;
                        println!("Loaded {} capsules", self.capsules.len());
                        self.update_library_labels();
                        self.rebuild_games_list(sender.clone());
                    }
                    Err(e) => {
                        eprintln!("Failed to load capsules: {}", e);
                    }
                }
            }
            MainWindowMsg::OpenAddGame => {
                println!("Open add game dialog");
                self.open_add_game_dialog(sender);
            }
            MainWindowMsg::AddGameModeChosen(mode) => {
                self.add_game_dialog = None;
                self.pending_add_mode = Some(mode);
                self.open_game_path_dialog(sender, mode);
            }
            MainWindowMsg::GamePathSelected(path) => {
                self.game_path_dialog = None;
                self.pending_game_path = Some(path);
                self.open_name_dialog(sender);
            }
            MainWindowMsg::AddGameCancelled => {
                self.add_game_dialog = None;
                self.game_path_dialog = None;
                self.name_dialog = None;
                if let Some(dialog) = &self.umu_match_dialog {
                    dialog.close();
                }
                if let Some(dialog) = &self.existing_location_dialog {
                    dialog.close();
                }
                self.umu_match_dialog = None;
                self.existing_location_dialog = None;
                self.pending_add_mode = None;
                self.pending_game_path = None;
                self.pending_source_folder = None;
                self.pending_game_name = None;
                self.pending_game_id = None;
                self.pending_store = None;
                println!("Add game cancelled");
            }
            MainWindowMsg::ExistingSourceFolderSelected(path) => {
                self.game_path_dialog = None;
                self.pending_source_folder = Some(path);
                let game_name = match self.pending_game_name.clone() {
                    Some(name) => name,
                    None => {
                        eprintln!("No pending game name available");
                        return;
                    }
                };
                self.open_existing_game_location_dialog(sender, game_name);
            }
            MainWindowMsg::ExistingSourceFolderCancelled => {
                self.game_path_dialog = None;
                self.pending_source_folder = None;
                self.pending_game_id = None;
                self.pending_store = None;
                self.pending_game_name = None;
                self.pending_game_path = None;
            }
            MainWindowMsg::ExistingGameLocationConfirmed(folder) => {
                self.existing_location_dialog = None;
                self.finalize_existing_game(sender, folder);
            }
            MainWindowMsg::ExistingGameLocationCancelled => {
                self.existing_location_dialog = None;
                self.pending_source_folder = None;
                self.pending_game_id = None;
                self.pending_store = None;
                self.pending_game_name = None;
                self.pending_game_path = None;
            }
            MainWindowMsg::GameNameConfirmed(name) => {
                self.name_dialog = None;
                let name = Self::sanitize_name(&name);
                if name.is_empty() {
                    eprintln!("Game name cannot be empty");
                    return;
                }
                if self.pending_game_path.is_none() {
                    eprintln!("No game path selected");
                    return;
                }
                let add_mode = match self.pending_add_mode {
                    Some(mode) => mode,
                    None => {
                        eprintln!("Add game mode not set");
                        return;
                    }
                };

                self.pending_game_name = Some(name.clone());
                let matches = self.find_umu_matches(&name);
                if !matches.is_empty() {
                    self.open_umu_match_dialog(sender, name, matches);
                } else {
                    match add_mode {
                        AddGameMode::Installer => {
                            self.finalize_pending_game(sender, None, None);
                        }
                        AddGameMode::Existing => {
                            self.pending_game_id = None;
                            self.pending_store = None;
                            self.open_existing_source_folder_dialog(sender);
                        }
                    }
                }
            }
            MainWindowMsg::InstallerFinished { capsule_dir, success } => {
                self.preparing_installs.remove(&capsule_dir);
                self.active_installs.remove(&capsule_dir);
                if success {
                    let mut needs_exe = false;
                    let mut prompt_deps = false;
                    let mut deps_metadata: Option<CapsuleMetadata> = None;
                    match Capsule::load_from_dir(&capsule_dir) {
                        Ok(mut capsule) => {
                            needs_exe = capsule.metadata.executables.main.path.trim().is_empty();
                            if needs_exe {
                                if let Some(guess) = Self::guess_executable(&capsule) {
                                    capsule.metadata.executables.main.path =
                                        guess.path.to_string_lossy().to_string();
                                    capsule.metadata.executables.main.original_shortcut = guess
                                        .shortcut
                                        .map(|path| path.to_string_lossy().to_string());
                                    needs_exe = false;
                                }
                            }
                            capsule.metadata.install_state = InstallState::Installed;
                            prompt_deps = self.should_prompt_dependencies(&capsule.metadata);
                            deps_metadata = Some(capsule.metadata.clone());
                            if let Err(e) = capsule.save_metadata() {
                                eprintln!("Failed to update metadata: {}", e);
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to load capsule: {}", e);
                        }
                    }

                    if prompt_deps {
                        if needs_exe {
                            self.pending_settings_capsule = Some(capsule_dir.clone());
                        }
                        if let Some(metadata) = deps_metadata {
                            self.open_dependency_dialog(sender.clone(), capsule_dir.clone(), metadata);
                        }
                    } else if needs_exe {
                        self.open_game_settings_dialog(sender.clone(), capsule_dir.clone());
                    }
                    println!("Installer completed for {:?}", capsule_dir);
                } else {
                    eprintln!("Installer failed for {:?}", capsule_dir);
                }
                sender.input(MainWindowMsg::LoadCapsules);
            }
            MainWindowMsg::UmuDatabaseLoaded(entries) => {
                self.umu_entries = entries;
                self.umu_loaded = true;
                self.umu_load_error = None;
                println!("UMU database loaded ({} entries).", self.umu_entries.len());
            }
            MainWindowMsg::UmuDatabaseFailed(error) => {
                self.umu_loaded = true;
                self.umu_load_error = Some(error.clone());
                eprintln!("UMU database load failed: {}", error);
            }
            MainWindowMsg::UmuMatchChosen { game_id, store } => {
                match self.pending_add_mode {
                    Some(AddGameMode::Installer) => {
                        self.finalize_pending_game(sender, game_id, store);
                    }
                    Some(AddGameMode::Existing) => {
                        if self.pending_game_name.is_none() {
                            eprintln!("No pending game name for existing game");
                            return;
                        }
                        self.pending_game_id = game_id;
                        self.pending_store = store;
                        self.open_existing_source_folder_dialog(sender);
                    }
                    None => {
                        eprintln!("Add game mode not set");
                    }
                }
            }
            MainWindowMsg::UmuMatchDialogClosed => {
                self.umu_match_dialog = None;
            }
            MainWindowMsg::DependenciesSelected {
                capsule_dir,
                install_vcredist,
                install_dxweb,
                force,
            } => {
                match Capsule::load_from_dir(&capsule_dir) {
                    Ok(mut capsule) => {
                        capsule.metadata.install_vcredist = install_vcredist;
                        capsule.metadata.install_dxweb = install_dxweb;
                        if let Err(e) = capsule.save_metadata() {
                            eprintln!("Failed to update metadata: {}", e);
                        }
                        self.start_dependency_install(
                            sender.clone(),
                            capsule_dir,
                            capsule.metadata.clone(),
                            install_vcredist,
                            install_dxweb,
                            force,
                        );
                    }
                    Err(e) => {
                        eprintln!("Failed to load capsule: {}", e);
                    }
                }
            }
            MainWindowMsg::DependenciesFinished { capsule_dir, installed } => {
                self.dependency_installs.remove(&capsule_dir);
                match Capsule::load_from_dir(&capsule_dir) {
                    Ok(mut capsule) => {
                        let mut updated = false;
                        for dep in installed {
                            if !capsule.metadata.redistributables_installed.contains(&dep) {
                                capsule.metadata.redistributables_installed.push(dep);
                                updated = true;
                            }
                        }
                        if updated {
                            if let Err(e) = capsule.save_metadata() {
                                eprintln!("Failed to update metadata: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to load capsule: {}", e);
                    }
                }
                self.rebuild_games_list(sender.clone());
            }
            MainWindowMsg::DependenciesDialogClosed => {
                self.dependency_dialog = None;
                if let Some(capsule_dir) = self.pending_settings_capsule.take() {
                    self.open_game_settings_dialog(sender, capsule_dir);
                }
            }
            MainWindowMsg::LaunchGame(capsule_dir) => {
                if self.active_games.contains_key(&capsule_dir) {
                    return;
                }
                self.start_game(sender, capsule_dir);
            }
            MainWindowMsg::GameStarted { capsule_dir, pgid } => {
                self.active_games.insert(capsule_dir, pgid);
                self.rebuild_games_list(sender.clone());
            }
            MainWindowMsg::GameFinished { capsule_dir, success } => {
                self.active_games.remove(&capsule_dir);
                if success {
                    println!("Game finished for {:?}", capsule_dir);
                } else {
                    eprintln!("Game failed for {:?}", capsule_dir);
                }
                self.rebuild_games_list(sender.clone());
            }
            MainWindowMsg::InstallerStarted { capsule_dir, pgid } => {
                self.preparing_installs.remove(&capsule_dir);
                self.active_installs.insert(capsule_dir, pgid);
                self.rebuild_games_list(sender.clone());
            }
            MainWindowMsg::EditGame(capsule_dir) => {
                self.open_game_settings_dialog(sender, capsule_dir);
            }
            MainWindowMsg::SaveGameSettings {
                capsule_dir,
                exe_path,
                game_id,
                store,
                install_vcredist,
                install_dxweb,
                protonfixes_disable,
                xalia_enabled,
                protonfixes_tricks,
                protonfixes_replace_cmds,
                protonfixes_dxvk_sets,
            } => {
                match Capsule::load_from_dir(&capsule_dir) {
                    Ok(mut capsule) => {
                        if !exe_path.trim().is_empty() {
                            capsule.metadata.executables.main.path = exe_path;
                            capsule.metadata.install_state = InstallState::Installed;
                        } else {
                            capsule.metadata.executables.main.path = exe_path;
                        }
                        capsule.metadata.game_id = game_id;
                        capsule.metadata.store = store;
                        capsule.metadata.install_vcredist = install_vcredist;
                        capsule.metadata.install_dxweb = install_dxweb;
                        capsule.metadata.protonfixes_disable = protonfixes_disable;
                        capsule.metadata.xalia_enabled = xalia_enabled;
                        capsule.metadata.protonfixes_tricks = protonfixes_tricks;
                        capsule.metadata.protonfixes_replace_cmds = protonfixes_replace_cmds;
                        capsule.metadata.protonfixes_dxvk_sets = protonfixes_dxvk_sets;
                        if let Err(e) = capsule.save_metadata() {
                            eprintln!("Failed to update metadata: {}", e);
                        } else {
                            println!("Updated settings for {}", capsule.name);
                            sender.input(MainWindowMsg::LoadCapsules);
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to load capsule: {}", e);
                    }
                }
            }
            MainWindowMsg::SettingsDialogClosed => {
                self.settings_dialog = None;
            }
            MainWindowMsg::DeleteGame(capsule_dir) => {
                if let Err(e) = fs::remove_dir_all(&capsule_dir) {
                    eprintln!("Failed to delete capsule: {}", e);
                } else {
                    println!("Deleted capsule {:?}", capsule_dir);
                    sender.input(MainWindowMsg::LoadCapsules);
                }
            }
            MainWindowMsg::ResumeInstall(capsule_dir) => {
                match Capsule::load_from_dir(&capsule_dir) {
                    Ok(capsule) => {
                        let installer_path = capsule
                            .metadata
                            .installer_path
                            .as_ref()
                            .map(PathBuf::from);
                        if let Some(installer_path) = installer_path {
                            self.start_installer(
                                &sender,
                                capsule_dir,
                                capsule.metadata.clone(),
                                installer_path,
                            );
                            self.rebuild_games_list(sender.clone());
                        } else {
                            eprintln!("No installer path found for {}", capsule.name);
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to load capsule: {}", e);
                    }
                }
            }
            MainWindowMsg::KillInstall(capsule_dir) => {
                if let Some(pgid) = self.active_installs.remove(&capsule_dir) {
                    unsafe {
                        libc::kill(-pgid, libc::SIGKILL);
                    }
                    println!("Killed installer for {:?}", capsule_dir);
                    self.rebuild_games_list(sender.clone());
                }
            }
            MainWindowMsg::MarkInstallComplete(capsule_dir) => {
                match Capsule::load_from_dir(&capsule_dir) {
                    Ok(mut capsule) => {
                        capsule.metadata.install_state = InstallState::Installed;
                        if let Err(e) = capsule.save_metadata() {
                            eprintln!("Failed to update metadata: {}", e);
                            return;
                        }
                        if capsule.metadata.executables.main.path.trim().is_empty() {
                            self.open_game_settings_dialog(sender.clone(), capsule_dir);
                        }
                        sender.input(MainWindowMsg::LoadCapsules);
                    }
                    Err(e) => {
                        eprintln!("Failed to load capsule: {}", e);
                    }
                }
            }
            MainWindowMsg::OpenSystemSetup => {
                // Re-check system status before opening dialog
                self.system_check = SystemCheck::check();
                
                println!("Opening system setup dialog...");
                
                if let Some(dialog) = &self.system_setup_dialog {
                    dialog.emit(SystemSetupMsg::Refresh(self.system_check.clone()));
                    dialog.widget().present();
                } else {
                    let dialog = SystemSetupDialog::builder()
                        .launch(self.system_check.clone())
                        .forward(sender.input_sender(), MainWindowMsg::SystemSetupOutput);
                    dialog.widget().present();
                    self.system_setup_dialog = Some(dialog);
                }
            }
            MainWindowMsg::SystemSetupOutput(SystemSetupOutput::CloseRequested) => {
                if let Some(dialog) = &self.system_setup_dialog {
                    dialog.widget().close();
                }
            }
            MainWindowMsg::SystemSetupOutput(SystemSetupOutput::SystemCheckUpdated(system_check)) => {
                self.system_check = system_check;
            }
        }
    }

}
