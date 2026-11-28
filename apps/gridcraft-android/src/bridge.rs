//! File bridge between the Rust app and the Kotlin `MainActivity`.
//!
//! Android's Storage Access Framework (open and save pickers) is only reachable from Java/Kotlin.
//! Rather than calling it over JNI (which needs `unsafe` pointer handling), both sides exchange
//! small files in one folder inside the app's private storage:
//!
//! ```text
//! <root>/req/open-<id>.req      Rust → Kotlin: "show the open picker" (body = purpose)
//! <root>/req/save-<id>.req      Rust → Kotlin: "show the save picker" (body = suggested name)
//! <root>/req/save-<id>.bin      the bytes to write (created before the .req)
//! <root>/in/<id>__<name>        Kotlin → Rust: a picked file's bytes (renamed into place when complete)
//! <root>/in/<id>.cancel         Kotlin → Rust: the open picker was dismissed
//! <root>/res/save-<id>.res      Kotlin → Rust: `ok`, `cancel` or `fail: <reason>`
//! ```
//!
//! Every file is written under a temporary name and renamed, so a reader never sees half a file.
//! Pure `std`: it is unit-tested on the host.

use std::cell::Cell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// How a save ended on the Kotlin side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveResult {
    Saved,
    Cancelled,
    Failed(String),
}

/// What a poll found.
#[derive(Default, Debug)]
pub struct Polled {
    /// Files the user picked: (request id, name, bytes).
    pub files: Vec<(u64, String, Vec<u8>)>,
    /// Open pickers that were dismissed (their ids).
    pub cancelled_opens: Vec<u64>,
    /// Saves that ended: (id, result).
    pub saves: Vec<(u64, SaveResult)>,
}

pub struct Bridge {
    root: PathBuf,
    next: Cell<u64>,
}

/// Parses `"left top right bottom"`; anything malformed gives zeros, and values are clamped to sane sizes.
pub fn parse_insets(text: &str) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    let parts: Vec<f32> = text.split_whitespace().filter_map(|p| p.parse::<f32>().ok()).collect();
    if parts.len() == 4 {
        for (o, v) in out.iter_mut().zip(parts) {
            *o = if v.is_finite() { v.clamp(0.0, 4000.0) } else { 0.0 };
        }
    }
    out
}

/// A file name with any folder part and control characters removed.
pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base.chars().filter(|c| !c.is_control() && *c != ':').collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() { "document".to_string() } else { cleaned }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

impl Bridge {
    /// Creates the folders under `root` and clears leftovers from an earlier run.
    pub fn new(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        for sub in ["req", "in", "res"] {
            let dir = root.join(sub);
            if dir.exists() {
                fs::remove_dir_all(&dir)?;
            }
            fs::create_dir_all(&dir)?;
        }
        Ok(Self { root, next: Cell::new(1) })
    }

    /// System bar / cutout / keyboard insets in physical pixels (left, top, right, bottom), as last
    /// written by the Kotlin side to `<root>/../insets`. Zeros when unknown.
    pub fn insets(&self) -> [f32; 4] {
        let text = self.root.parent().map(|p| p.join("insets")).and_then(|p| fs::read_to_string(p).ok()).unwrap_or_default();
        parse_insets(&text)
    }

    fn take_id(&self) -> u64 {
        let id = self.next.get();
        self.next.set(id + 1);
        id
    }

    /// Asks for the open picker. `purpose` is `document`, `picture`, `font` or `recipients`.
    pub fn request_open(&self, purpose: &str) -> io::Result<u64> {
        let id = self.take_id();
        write_atomic(&self.root.join("req").join(format!("open-{id}.req")), purpose.as_bytes())?;
        Ok(id)
    }

    /// Asks for the save picker with a suggested file name; the bytes are written first so the
    /// Kotlin side can copy them as soon as the request appears.
    pub fn request_save(&self, name: &str, bytes: &[u8]) -> io::Result<u64> {
        let id = self.take_id();
        let req = self.root.join("req");
        write_atomic(&req.join(format!("save-{id}.bin")), bytes)?;
        write_atomic(&req.join(format!("save-{id}.req")), safe_name(name).as_bytes())?;
        Ok(id)
    }

    /// Collects what the Kotlin side has finished since the last call.
    pub fn poll(&self) -> Polled {
        let mut out = Polled::default();
        if let Ok(entries) = fs::read_dir(self.root.join("in")) {
            let mut names: Vec<String> = entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            names.sort();
            for name in names {
                if name.ends_with(".tmp") {
                    continue;
                }
                let path = self.root.join("in").join(&name);
                if let Some(id) = name.strip_suffix(".cancel").and_then(|s| s.parse().ok()) {
                    out.cancelled_opens.push(id);
                } else if let Some((id, file)) = name.split_once("__").and_then(|(id, file)| Some((id.parse::<u64>().ok()?, file))) {
                    match fs::read(&path) {
                        Ok(bytes) => out.files.push((id, file.to_string(), bytes)),
                        Err(e) => log::error!("couldn't read picked file {name}: {e}"),
                    }
                }
                let _ = fs::remove_file(&path);
            }
        }
        if let Ok(entries) = fs::read_dir(self.root.join("res")) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let Some(id) = name.strip_prefix("save-").and_then(|s| s.strip_suffix(".res")).and_then(|s| s.parse::<u64>().ok()) else {
                    continue;
                };
                let body = fs::read_to_string(e.path()).unwrap_or_default();
                let result = match body.trim() {
                    "ok" => SaveResult::Saved,
                    "cancel" => SaveResult::Cancelled,
                    other => SaveResult::Failed(other.strip_prefix("fail:").unwrap_or(other).trim().to_string()),
                };
                out.saves.push((id, result));
                let _ = fs::remove_file(e.path());
                let _ = fs::remove_file(self.root.join("req").join(format!("save-{id}.bin")));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn insets_parse_and_reject_junk() {
        assert_eq!(super::parse_insets("0 96 0 144"), [0.0, 96.0, 0.0, 144.0]);
        assert_eq!(super::parse_insets("1 2 3"), [0.0; 4]);
        assert_eq!(super::parse_insets("a b c d"), [0.0; 4]);
        assert_eq!(super::parse_insets("-5 1e9 0 0"), [0.0, 4000.0, 0.0, 0.0]);
    }

    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gridcraft-bridge-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn open_request_and_picked_file_roundtrip() {
        let root = scratch("open");
        let b = Bridge::new(&root).expect("bridge");
        let id = b.request_open("document").expect("request");
        assert_eq!(fs::read_to_string(root.join("req").join(format!("open-{id}.req"))).expect("req"), "document");
        fs::write(root.join("in").join(format!("{id}__letter.docx")), b"PK").expect("write");
        fs::write(root.join("in").join(format!("{id}__half.docx.tmp")), b"x").expect("write");
        let polled = b.poll();
        assert_eq!(polled.files, vec![(id, "letter.docx".to_string(), b"PK".to_vec())]);
        assert!(b.poll().files.is_empty(), "consumed files are removed");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cancelled_open_is_reported() {
        let root = scratch("cancel");
        let b = Bridge::new(&root).expect("bridge");
        fs::write(root.join("in").join("7.cancel"), b"").expect("write");
        assert_eq!(b.poll().cancelled_opens, vec![7]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn save_request_and_results() {
        let root = scratch("save");
        let b = Bridge::new(&root).expect("bridge");
        let id = b.request_save("../evil/Report.docx", b"data").expect("request");
        assert_eq!(fs::read_to_string(root.join("req").join(format!("save-{id}.req"))).expect("req"), "Report.docx");
        assert_eq!(fs::read(root.join("req").join(format!("save-{id}.bin"))).expect("bin"), b"data");
        for (body, want) in
            [("ok", SaveResult::Saved), ("cancel", SaveResult::Cancelled), ("fail: disk full", SaveResult::Failed("disk full".into()))]
        {
            fs::write(root.join("res").join(format!("save-{id}.res")), body).expect("write");
            assert_eq!(b.poll().saves, vec![(id, want)]);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn names_are_sanitised() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("C:\\x\\a.docx"), "a.docx");
        assert_eq!(safe_name("..."), "document");
        assert_eq!(safe_name(""), "document");
    }
}
