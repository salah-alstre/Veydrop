//! Streaming "stored" (uncompressed) ZIP writer.
//!
//! Because entries are stored and their CRC-32 is known up front, the exact
//! archive size is known before the first byte is sent, so the HTTP response
//! can carry a real `Content-Length` (browsers then show true download
//! progress). No ZIP64: selections of 4 GiB or more are refused with
//! `Error::ZipTooLarge` and the phone is told to download files individually.

use std::path::PathBuf;
use std::time::SystemTime;

use bytes::Bytes;
use futures_util::Stream;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::fsutil;

const LOCAL_SIG: u32 = 0x0403_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const EOCD_SIG: u32 = 0x0605_4b50;
const LIMIT: u64 = u32::MAX as u64;
const CHUNK: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct ZipEntry {
    /// Path inside the archive ('/' separated, re-sanitised on use).
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub crc32: u32,
    pub mtime: SystemTime,
}

struct Prepared {
    name: Vec<u8>,
    entry: ZipEntry,
    dos_time: u16,
    dos_date: u16,
    offset: u32,
}

fn clean_name(raw: &str) -> String {
    let parts = fsutil::sanitize_rel_path(raw);
    parts.join("/")
}

fn dos_datetime(t: SystemTime) -> (u16, u16) {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    if month <= 2 {
        year += 1;
    }
    if year < 1980 {
        return (0, (1 << 5) | 1); // 00:00:00, 1980-01-01
    }
    let year = year.min(2107);
    let time = ((rem / 3600) << 11) | (((rem % 3600) / 60) << 5) | ((rem % 60) / 2);
    let date = ((year - 1980) << 9) | (month << 5) | day;
    (time as u16, date as u16)
}

fn prepare(entries: &[ZipEntry]) -> Result<(Vec<Prepared>, u64)> {
    if entries.is_empty() {
        return Err(Error::invalid("empty selection"));
    }
    if entries.len() > u16::MAX as usize {
        return Err(Error::ZipTooLarge);
    }
    let mut out = Vec::with_capacity(entries.len());
    let mut offset: u64 = 0;
    let mut central: u64 = 0;
    for e in entries {
        if e.size >= LIMIT {
            return Err(Error::ZipTooLarge);
        }
        let name = clean_name(&e.name);
        if name.is_empty() {
            return Err(Error::invalid("empty entry name"));
        }
        let name = name.into_bytes();
        if name.len() > u16::MAX as usize {
            return Err(Error::ZipTooLarge);
        }
        if offset >= LIMIT {
            return Err(Error::ZipTooLarge);
        }
        let (dos_time, dos_date) = dos_datetime(e.mtime);
        let nlen = name.len() as u64;
        out.push(Prepared { name, entry: e.clone(), dos_time, dos_date, offset: offset as u32 });
        offset += 30 + nlen + e.size;
        central += 46 + nlen;
    }
    let total = offset + central + 22;
    if total >= LIMIT {
        return Err(Error::ZipTooLarge);
    }
    Ok((out, total))
}

/// Exact byte length of the archive, or an error if it can't be represented.
pub fn content_length(entries: &[ZipEntry]) -> Result<u64> {
    prepare(entries).map(|(_, total)| total)
}

fn local_header(p: &Prepared) -> Vec<u8> {
    let mut h = Vec::with_capacity(30 + p.name.len());
    h.extend_from_slice(&LOCAL_SIG.to_le_bytes());
    h.extend_from_slice(&20u16.to_le_bytes()); // version needed
    h.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
    h.extend_from_slice(&0u16.to_le_bytes()); // stored
    h.extend_from_slice(&p.dos_time.to_le_bytes());
    h.extend_from_slice(&p.dos_date.to_le_bytes());
    h.extend_from_slice(&p.entry.crc32.to_le_bytes());
    h.extend_from_slice(&(p.entry.size as u32).to_le_bytes());
    h.extend_from_slice(&(p.entry.size as u32).to_le_bytes());
    h.extend_from_slice(&(p.name.len() as u16).to_le_bytes());
    h.extend_from_slice(&0u16.to_le_bytes()); // extra
    h.extend_from_slice(&p.name);
    h
}

fn central_header(p: &Prepared) -> Vec<u8> {
    let mut h = Vec::with_capacity(46 + p.name.len());
    h.extend_from_slice(&CENTRAL_SIG.to_le_bytes());
    h.extend_from_slice(&20u16.to_le_bytes()); // made by
    h.extend_from_slice(&20u16.to_le_bytes()); // needed
    h.extend_from_slice(&0x0800u16.to_le_bytes());
    h.extend_from_slice(&0u16.to_le_bytes());
    h.extend_from_slice(&p.dos_time.to_le_bytes());
    h.extend_from_slice(&p.dos_date.to_le_bytes());
    h.extend_from_slice(&p.entry.crc32.to_le_bytes());
    h.extend_from_slice(&(p.entry.size as u32).to_le_bytes());
    h.extend_from_slice(&(p.entry.size as u32).to_le_bytes());
    h.extend_from_slice(&(p.name.len() as u16).to_le_bytes());
    h.extend_from_slice(&0u16.to_le_bytes()); // extra
    h.extend_from_slice(&0u16.to_le_bytes()); // comment
    h.extend_from_slice(&0u16.to_le_bytes()); // disk
    h.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
    h.extend_from_slice(&0u32.to_le_bytes()); // external attrs
    h.extend_from_slice(&p.offset.to_le_bytes());
    h.extend_from_slice(&p.name);
    h
}

fn eocd(count: u16, cd_size: u32, cd_offset: u32) -> Vec<u8> {
    let mut h = Vec::with_capacity(22);
    h.extend_from_slice(&EOCD_SIG.to_le_bytes());
    h.extend_from_slice(&0u16.to_le_bytes());
    h.extend_from_slice(&0u16.to_le_bytes());
    h.extend_from_slice(&count.to_le_bytes());
    h.extend_from_slice(&count.to_le_bytes());
    h.extend_from_slice(&cd_size.to_le_bytes());
    h.extend_from_slice(&cd_offset.to_le_bytes());
    h.extend_from_slice(&0u16.to_le_bytes());
    h
}

/// Streams the archive. `on_bytes` receives the number of bytes just
/// produced (headers included) so callers can drive a progress meter. The
/// stream errors out — rather than emitting a silently corrupt archive — if a
/// source file changes while it is being read.
pub fn stream(
    entries: Vec<ZipEntry>,
    on_bytes: impl Fn(u64) + Send + 'static,
    cancel: CancellationToken,
) -> Result<(u64, impl Stream<Item = std::io::Result<Bytes>>)> {
    let (prepared, total) = prepare(&entries)?;
    let s = async_stream::try_stream! {
        let mut cd_size: u64 = 0;
        let mut sent: u64 = 0;
        let mut buf = vec![0u8; CHUNK];
        for p in &prepared {
            let h = local_header(p);
            sent += h.len() as u64;
            on_bytes(h.len() as u64);
            yield Bytes::from(h);

            let mut file = tokio::fs::File::open(&p.entry.path).await?;
            let mut remaining = p.entry.size;
            let mut crc = crc32fast::Hasher::new();
            while remaining > 0 {
                if cancel.is_cancelled() {
                    Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"))?;
                }
                let want = remaining.min(CHUNK as u64) as usize;
                let n = file.read(&mut buf[..want]).await?;
                if n == 0 {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "source file shrank while being archived",
                    ))?;
                }
                crc.update(&buf[..n]);
                remaining -= n as u64;
                sent += n as u64;
                on_bytes(n as u64);
                yield Bytes::copy_from_slice(&buf[..n]);
            }
            // The size/CRC in the header were promised up front; verify them.
            let mut probe = [0u8; 1];
            if file.read(&mut probe).await? != 0 || crc.finalize() != p.entry.crc32 {
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "source file changed while being archived",
                ))?;
            }
        }
        let cd_offset = sent;
        for p in &prepared {
            let h = central_header(p);
            cd_size += h.len() as u64;
            yield Bytes::from(h);
        }
        let tail = eocd(prepared.len() as u16, cd_size as u32, cd_offset as u32);
        yield Bytes::from(tail);
    };
    Ok((total, s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use std::io::{Cursor, Read};

    fn entry(dir: &std::path::Path, name: &str, data: &[u8]) -> ZipEntry {
        let path = dir.join(name.replace('/', "_"));
        std::fs::write(&path, data).unwrap();
        ZipEntry {
            name: name.to_string(),
            path,
            size: data.len() as u64,
            crc32: crc32fast::hash(data),
            mtime: SystemTime::now(),
        }
    }

    async fn collect(entries: Vec<ZipEntry>) -> (u64, Vec<u8>) {
        let (len, s) = stream(entries, |_| {}, CancellationToken::new()).unwrap();
        futures_util::pin_mut!(s);
        let mut out = Vec::new();
        while let Some(chunk) = s.next().await {
            out.extend_from_slice(&chunk.unwrap());
        }
        (len, out)
    }

    #[tokio::test]
    async fn archive_is_valid_and_length_is_exact() {
        let dir = tempfile::tempdir().unwrap();
        let big: Vec<u8> = (0..600_000u32).map(|i| (i % 251) as u8).collect();
        let es = vec![
            entry(dir.path(), "Folder/a.txt", b"hello"),
            entry(dir.path(), "Folder/sub/ملف.bin", &big),
            entry(dir.path(), "empty.txt", b""),
        ];
        let (len, bytes) = collect(es).await;
        assert_eq!(len as usize, bytes.len());

        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(z.len(), 3);
        let mut s = String::new();
        z.by_name("Folder/a.txt").unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "hello");
        let mut v = Vec::new();
        z.by_name("Folder/sub/ملف.bin").unwrap().read_to_end(&mut v).unwrap();
        assert_eq!(v, big);
        assert_eq!(z.by_name("empty.txt").unwrap().size(), 0);
    }

    #[tokio::test]
    async fn hostile_names_cannot_escape() {
        let dir = tempfile::tempdir().unwrap();
        let es = vec![entry(dir.path(), "../../evil.txt", b"x")];
        let (_, bytes) = collect(es).await;
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        for i in 0..z.len() {
            let f = z.by_index(i).unwrap();
            assert!(!f.name().contains(".."), "name was {}", f.name());
            assert!(!f.name().starts_with('/'));
        }
    }

    #[tokio::test]
    async fn changed_source_fails_the_stream() {
        let dir = tempfile::tempdir().unwrap();
        let e = entry(dir.path(), "a.txt", b"hello world");
        std::fs::write(&e.path, b"HELLO WORLD").unwrap(); // same size, new content
        let (_, s) = stream(vec![e], |_| {}, CancellationToken::new()).unwrap();
        futures_util::pin_mut!(s);
        let mut failed = false;
        while let Some(c) = s.next().await {
            if c.is_err() {
                failed = true;
                break;
            }
        }
        assert!(failed);
    }

    #[test]
    fn too_large_is_refused() {
        let e = ZipEntry {
            name: "big".into(),
            path: "x".into(),
            size: 5 * 1024 * 1024 * 1024,
            crc32: 0,
            mtime: SystemTime::now(),
        };
        assert!(matches!(content_length(&[e]), Err(Error::ZipTooLarge)));
    }

    #[test]
    fn dos_date_clamps_to_1980() {
        let (t, d) = dos_datetime(SystemTime::UNIX_EPOCH);
        assert_eq!((t, d), (0, 33));
    }
}
