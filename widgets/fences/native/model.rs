use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const RULE_EXTS_IMAGES: &str =
    ".png, .jpg, .jpeg, .webp, .gif, .bmp, .svg, .ico, .avif, .heic, .tiff";
pub const RULE_EXTS_PROGRAMS: &str = ".exe, .msi, .lnk, .bat, .cmd, .appref-ms";
pub const RULE_EXTS_DOWNLOADS: &str =
    ".zip, .rar, .7z, .tar, .gz, .pdf, .doc, .docx, .xls, .xlsx, .ppt, .pptx, .txt, .csv, .mp4, .mkv, .mp3, .wav, .flac, .apk, .iso, .torrent";

pub fn default_rule_for_fence(id: &str, title: &str) -> (bool, String) {
    let key = id.to_ascii_lowercase();
    let t = title.to_ascii_lowercase();
    if key == "images" || t.contains("image") || t.contains("photo") {
        (true, RULE_EXTS_IMAGES.into())
    } else if key == "pc-programs" || t.contains("program") || t.contains("app") {
        (true, RULE_EXTS_PROGRAMS.into())
    } else if key == "downloads" || t.contains("download") {
        (true, RULE_EXTS_DOWNLOADS.into())
    } else {
        (false, String::new())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FenceSpec {
    pub id: String,
    pub title: String,
    pub folder: String,
    pub position: [i32; 2],
    pub size: [i32; 2],
    pub fade_on_hover: bool,
    pub idle_opacity: f32,
    pub rolled_up: bool,
    pub sort_by: String,
    pub sort_desc: bool,
    pub icon_size: i32,
    pub text_size: i32,
    pub view_mode: String,
    pub show_thumbnails: bool,
    pub custom_order: Vec<String>,
    pub filter: String,
    pub folders_first: bool,
    pub visible: bool,
    pub auto_rule_enabled: bool,
    pub auto_rule_exts: String,
}

impl Default for FenceSpec {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: "Fence".into(),
            folder: String::new(),
            position: [40, 40],
            size: [340, 240],
            fade_on_hover: true,
            idle_opacity: 0.20,
            rolled_up: false,
            sort_by: "name".into(),
            sort_desc: false,
            icon_size: 32,
            text_size: 13,
            view_mode: "grid".into(),
            show_thumbnails: true,
            custom_order: Vec::new(),
            filter: "all".into(),
            folders_first: true,
            visible: true,
            auto_rule_enabled: false,
            auto_rule_exts: String::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FencesConfig {
    pub open: bool,
    pub rules_enabled: bool,
    pub watch_downloads: bool,
    pub rules_initialized: bool,
    pub fences: Vec<FenceSpec>,
}

impl Default for FencesConfig {
    fn default() -> Self {
        Self {
            open: true,
            rules_enabled: true,
            watch_downloads: true,
            rules_initialized: true,
            fences: default_fences(),
        }
    }
}

pub fn user_home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Users\Public"))
}

fn default_fences() -> Vec<FenceSpec> {
    let home = user_home();
    let desk = home.join("Desktop");
    let downloads = home.join("Downloads");
    let candidates = [
        ("web", "Web", desk.join("Web"), [20, 740], [340, 250], false, "name", false),
        ("games", "Games", desk.join("Games"), [376, 740], [340, 250], false, "name", false),
        ("ai", "AI", desk.join("AI"), [732, 740], [420, 250], false, "name", false),
        ("downloads", "Downloads", downloads, [1540, 20], [360, 340], true, "date", true),
        ("ssh-dev", "SSH & Dev", desk.join("SSH & Dev"), [1540, 376], [360, 320], true, "name", false),
        ("pc-programs", "PC Programs", desk.join("PC Programs"), [1168, 712], [732, 280], true, "name", false),
        ("tarot", "Tarot & HNHH", desk.join("Tarot & HNHH"), [20, 20], [380, 340], true, "name", false),
        ("images", "Images", desk.join("Images"), [20, 376], [380, 340], true, "name", false),
    ];
    let mut out = Vec::new();
    for (id, title, path, pos, size, fade, sort_by, sort_desc) in candidates {
        if path.is_dir() {
            let is_img = id == "images";
            let (auto_rule_enabled, auto_rule_exts) = default_rule_for_fence(id, title);
            out.push(FenceSpec {
                id: id.into(),
                title: title.into(),
                folder: path.to_string_lossy().into_owned(),
                position: pos,
                size,
                fade_on_hover: fade,
                idle_opacity: 0.20,
                rolled_up: false,
                sort_by: sort_by.into(),
                sort_desc,
                icon_size: if is_img { 48 } else { 32 },
                text_size: 13,
                view_mode: if is_img { "gallery".into() } else { "grid".into() },
                show_thumbnails: true,
                custom_order: Vec::new(),
                filter: "all".into(),
                folders_first: true,
                visible: true,
                auto_rule_enabled,
                auto_rule_exts,
            });
        }
    }
    if out.is_empty() {
        out.push(FenceSpec {
            id: "desktop".into(),
            title: "Desktop".into(),
            folder: desk.to_string_lossy().into_owned(),
            position: [40, 40],
            size: [420, 320],
            fade_on_hover: true,
            idle_opacity: 0.20,
            rolled_up: false,
            sort_by: "name".into(),
            sort_desc: false,
            icon_size: 32,
            text_size: 13,
            view_mode: "grid".into(),
            show_thumbnails: true,
            custom_order: Vec::new(),
            filter: "all".into(),
            folders_first: true,
            visible: true,
            auto_rule_enabled: false,
            auto_rule_exts: String::new(),
        });
    }
    out
}

impl FencesConfig {
    pub fn path(root: &Path) -> PathBuf {
        root.join("config").join("fences.json")
    }

    pub fn load(root: &Path) -> Self {
        let path = Self::path(root);
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(mut cfg) = serde_json::from_slice::<Self>(&bytes) {
                if !cfg.rules_initialized {
                    cfg.rules_initialized = true;
                    cfg.rules_enabled = true;
                    cfg.watch_downloads = true;
                    for f in &mut cfg.fences {
                        if f.auto_rule_exts.trim().is_empty() {
                            let (en, exts) = default_rule_for_fence(&f.id, &f.title);
                            if en {
                                f.auto_rule_enabled = true;
                                f.auto_rule_exts = exts;
                            }
                        }
                    }
                    let _ = cfg.save(root);
                }
                return cfg;
            }
        }
        let cfg = Self::default();
        let _ = cfg.save(root);
        cfg
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let dir = root.join("config");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let target = Self::path(root);
        let temp = dir.join("fences.json.tmp");
        let json = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&temp, json).map_err(|e| e.to_string())?;
        unsafe {
            use windows_sys::Win32::Storage::FileSystem::*;
            if MoveFileExW(
                crate::model::wide(&temp).as_ptr(),
                crate::model::wide(&target).as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        Ok(())
    }
}
