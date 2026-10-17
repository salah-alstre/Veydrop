//! File-system helpers: name sanitisation, collision-free naming, atomic
//! writes and free-space queries. Everything that turns remote input into a
//! local path goes through here.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const MAX_COMPONENT_BYTES: usize = 180;
const MAX_DEPTH: usize = 32;

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}')
}

/// Turns an arbitrary, untrusted string into a single safe Windows path
/// component. Never returns an empty string, "." or "..".
pub fn sanitize_component(input: &str) -> String {
    let mapped: String = input
        .chars()
        .filter(|c| !is_bidi_control(*c))
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 || c as u32 == 0x7f => '_',
            c => c,
        })
        .collect();
    // Windows silently strips trailing dots/spaces, which can alias names.
    let mut s = mapped.trim_end_matches(['.', ' ']).trim_start_matches(' ').to_string();
    if s.is_empty() {
        return "file".into();
    }
    let stem_end = s.find('.').unwrap_or(s.len());
    let stem = s[..stem_end].trim_end_matches(' ');
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        s = format!("_{s}");
    }
    truncate_name(&s, MAX_COMPONENT_BYTES)
}

/// Truncates to `max` bytes on a char boundary while keeping the extension.
fn truncate_name(name: &str, max: usize) -> String {
    if name.len() <= max {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && name.len() - i <= 16 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let budget = max.saturating_sub(ext.len());
    let mut end = budget.min(stem.len());
    while end > 0 && !stem.is_char_boundary(end) {
        end -= 1;
    }
    let out = format!("{}{}", stem[..end].trim_end_matches(['.', ' ']), ext);
    if out.is_empty() {
        "file".into()
    } else {
        out
    }
}

/// Splits an untrusted relative path into safe components. `..`, `.`, empty
/// parts and drive prefixes can never survive, so the result is always
/// contained in whatever directory it is joined onto.
pub fn sanitize_rel_path(input: &str) -> Vec<String> {
    let mut out: Vec<String> = input
        .split(['/', '\\'])
        .filter(|p| !p.is_empty() && *p != "." && *p != "..")
        .map(sanitize_component)
        .collect();
    if out.len() > MAX_DEPTH {
        out = out.split_off(out.len() - MAX_DEPTH);
    }
    out
}

fn numbered(name: &str, n: u32) -> String {
    if n == 0 {
        return name.to_string();
    }
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({}){}", &name[..i], n, &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

/// Atomically reserves a unique file name in `dir` by creating an empty file
/// with `create_new`. The caller later renames its finished download over
/// the placeholder, so two transfers can never claim the same name.
pub fn reserve_unique_file(dir: &Path, name: &str) -> io::Result<PathBuf> {
    for n in 0..10_000u32 {
        let candidate = dir.join(numbered(name, n));
        match OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(_) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "could not find a free file name"))
}

/// Atomically reserves a unique directory name (used for received folders).
pub fn reserve_unique_dir(parent: &Path, name: &str) -> io::Result<PathBuf> {
    for n in 0..10_000u32 {
        let candidate = parent.join(numbered(name, n));
        match std::fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "could not find a free folder name"))
}

/// Writes `data` to `path` via a temp file + rename so a crash can never
/// leave a half-written settings/history file behind.
pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(windows)]
pub fn available_space(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut p = path;
    // Walk up to an existing directory.
    while !p.exists() {
        p = p.parent()?;
    }
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free: u64 = 0;
    // SAFETY: `wide` is NUL-terminated and outlives the call; the out-pointer
    // is valid for writes and the other two may be null per the API contract.
    let ok = unsafe {
        GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut())
    };
    if ok != 0 {
        Some(free)
    } else {
        None
    }
}

#[cfg(not(windows))]
pub fn available_space(_path: &Path) -> Option<u64> {
    None
}

/// Moves `from` to `to`, falling back to copy + delete across volumes.
pub fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_dangerous_names() {
        assert_eq!(sanitize_component("a<b>c:d\"e|f?g*h"), "a_b_c_d_e_f_g_h");
        assert_eq!(sanitize_component(".."), "file");
        assert_eq!(sanitize_component("   "), "file");
        assert_eq!(sanitize_component("name. . ."), "name");
        assert_eq!(sanitize_component("CON"), "_CON");
        assert_eq!(sanitize_component("nul.txt"), "_nul.txt");
        assert_eq!(sanitize_component("photo\u{202E}gpj.exe"), "photogpj.exe");
        assert_eq!(sanitize_component("report.pdf:evil.exe"), "report.pdf_evil.exe");
    }

    #[test]
    fn traversal_paths_are_neutralised() {
        assert_eq!(sanitize_rel_path("../../Windows/system32/x.dll"), vec!["Windows", "system32", "x.dll"]);
        assert_eq!(sanitize_rel_path("..\\..\\x"), vec!["x"]);
        assert_eq!(sanitize_rel_path("C:\\Windows\\x"), vec!["C_", "Windows", "x"]);
        assert_eq!(sanitize_rel_path("/etc/passwd"), vec!["etc", "passwd"]);
        assert!(sanitize_rel_path("../..").is_empty());
    }

    #[test]
    fn long_names_keep_extension() {
        let n = format!("{}.jpeg", "é".repeat(300));
        let s = sanitize_component(&n);
        assert!(s.len() <= 180);
        assert!(s.ends_with(".jpeg"));
    }

    #[test]
    fn unique_names_do_not_collide() {
        let d = tempfile::tempdir().unwrap();
        let a = reserve_unique_file(d.path(), "a.txt").unwrap();
        let b = reserve_unique_file(d.path(), "a.txt").unwrap();
        let c = reserve_unique_file(d.path(), "a.txt").unwrap();
        assert_eq!(a.file_name().unwrap(), "a.txt");
        assert_eq!(b.file_name().unwrap(), "a (1).txt");
        assert_eq!(c.file_name().unwrap(), "a (2).txt");
        let d1 = reserve_unique_dir(d.path(), "Pics").unwrap();
        let d2 = reserve_unique_dir(d.path(), "Pics").unwrap();
        assert_ne!(d1, d2);
    }
}
