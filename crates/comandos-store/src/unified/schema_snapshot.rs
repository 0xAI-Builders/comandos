//! Bounded schema-only sparse snapshot. The caller copies the complete WAL,
//! checks source path identity/WAL stability, and lets SQLite validate schema.
//! Any error requires discarding this staging file and falling back to a fresh
//! full snapshot. This module does not open SQLite on the original database.

use crate::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, Metadata, OpenOptions};
use std::io;
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

const MAX_PAGES: usize = 4096;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_WAL_FRAMES: u64 = 1_000_000;
const MAX_PAGE_NUMBER: u32 = u32::MAX - 1;

fn invalid(message: &str) -> Error {
    Error::Validation(format!("sparse schema snapshot: {message}"))
}
fn be16(bytes: &[u8], offset: usize) -> Result<u16> {
    let bytes = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| invalid("truncated integer"))?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}
fn be32(bytes: &[u8], offset: usize) -> Result<u32> {
    let bytes = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| invalid("truncated integer"))?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
fn read_at(file: &File, bytes: &mut [u8], offset: u64) -> Result<()> {
    file.read_exact_at(bytes, offset).map_err(Error::Io)
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct Stamp {
    dev: u64,
    ino: u64,
    len: u64,
    mode: u32,
    mtime: (i64, i64),
    ctime: (i64, i64),
}
impl Stamp {
    fn of(metadata: &Metadata) -> Self {
        Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
            len: metadata.len(),
            mode: metadata.mode(),
            mtime: (metadata.mtime(), metadata.mtime_nsec()),
            ctime: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }
}

fn page_geometry(header: &[u8]) -> Result<(usize, usize)> {
    if header.get(..16) != Some(b"SQLite format 3\0".as_slice()) {
        return Err(invalid("unsupported database header"));
    }
    let encoded = be16(header, 16)?;
    let size = if encoded == 1 {
        65536
    } else {
        usize::from(encoded)
    };
    if !(512..=65536).contains(&size) || !size.is_power_of_two() {
        return Err(invalid("unsupported database page size"));
    }
    if !matches!(header[18], 1 | 2)
        || !matches!(header[19], 1 | 2)
        || header[21..24] != [64, 32, 32]
    {
        return Err(invalid("unsupported database format"));
    }
    let usable = size - usize::from(header[20]);
    if usable < 480 {
        return Err(invalid("unsupported reserved page space"));
    }
    Ok((size, usable))
}

/// Rolling WAL checksum over pairs of 32-bit words. Header checksum fields
/// themselves are always stored big-endian; magic selects word byte order.
fn checksum(bytes: &[u8], big_endian: bool, mut sum: [u32; 2]) -> [u32; 2] {
    for pair in bytes.chunks_exact(8) {
        let a = [pair[0], pair[1], pair[2], pair[3]];
        let b = [pair[4], pair[5], pair[6], pair[7]];
        let (a, b) = if big_endian {
            (u32::from_be_bytes(a), u32::from_be_bytes(b))
        } else {
            (u32::from_le_bytes(a), u32::from_le_bytes(b))
        };
        sum[0] = sum[0].wrapping_add(a).wrapping_add(sum[1]);
        sum[1] = sum[1].wrapping_add(b).wrapping_add(sum[0]);
    }
    sum
}

struct Wal {
    file: File,
    stamp: Stamp,
    pages: BTreeMap<u32, u64>,
    database_pages: Option<u32>,
}
impl Wal {
    fn open(path: &Path, page_size: usize) -> Result<Option<Self>> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(Error::Io(error)),
        };
        let metadata = file.metadata().map_err(Error::Io)?;
        if !metadata.is_file() || metadata.len() < 32 {
            return Err(invalid("empty or truncated WAL header"));
        }
        let stamp = Stamp::of(&metadata);
        let mut header = [0; 32];
        read_at(&file, &mut header, 0)?;
        let magic = be32(&header, 0)?;
        if !matches!(magic, 0x377f_0682 | 0x377f_0683)
            || be32(&header, 4)? != 3_007_000
            || u64::from(be32(&header, 8)?) != page_size as u64
        {
            return Err(invalid("unsupported WAL header"));
        }
        let big_endian = magic & 1 != 0;
        let mut sum = checksum(&header[..24], big_endian, [0, 0]);
        if sum != [be32(&header, 24)?, be32(&header, 28)?] {
            return Err(invalid("invalid WAL header checksum"));
        }
        let frame_size = page_size as u64 + 24;
        let frames = (metadata.len() - 32) / frame_size;
        if frames > MAX_WAL_FRAMES {
            return Err(invalid("WAL exceeds bounded navigation limit"));
        }
        let mut pending = BTreeMap::new();
        let mut pages = BTreeMap::new();
        let mut database_pages = None;
        let mut frame_header = [0; 24];
        let mut page = vec![0; page_size];
        for frame in 0..frames {
            let offset = 32 + frame * frame_size;
            read_at(&file, &mut frame_header, offset)?;
            let number = be32(&frame_header, 0)?;
            if number == 0 || number > MAX_PAGE_NUMBER || frame_header[8..16] != header[16..24] {
                break;
            }
            read_at(&file, &mut page, offset + 24)?;
            let next = checksum(&frame_header[..8], big_endian, sum);
            let next = checksum(&page, big_endian, next);
            if next != [be32(&frame_header, 16)?, be32(&frame_header, 20)?] {
                break;
            }
            sum = next;
            pending.insert(number, offset + 24);
            let size = be32(&frame_header, 4)?;
            if size != 0 {
                if size > MAX_PAGE_NUMBER {
                    return Err(invalid("unsupported committed database size"));
                }
                // Do not discard older mappings across truncation: SQLite
                // searches the last frame <= the final committed frame.
                pages.append(&mut pending);
                database_pages = Some(size);
            }
        }
        // Pending frames after the last commit and invalid/trailing frames
        // are invisible, exactly as in SQLite's WAL recovery.
        if Stamp::of(&file.metadata().map_err(Error::Io)?) != stamp {
            return Err(invalid("private WAL changed during navigation"));
        }
        Ok(Some(Self {
            file,
            stamp,
            pages,
            database_pages,
        }))
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Family {
    Table,
    Index,
}

pub(super) struct SchemaSnapshot {
    base: File,
    staged: File,
    initial: Stamp,
    wal: Option<Wal>,
    page_size: usize,
    usable: usize,
    physical_pages: u32,
    database_pages: u32,
    limit: usize,
    originals: BTreeMap<u32, Vec<u8>>,
    visited: BTreeSet<u32>,
    roots: BTreeSet<u32>,
    poisoned: bool,
}

pub(super) fn prepare(base: File, staged: &Path, copied_wal: &Path) -> Result<SchemaSnapshot> {
    let metadata = base.metadata().map_err(Error::Io)?;
    if !metadata.is_file() || metadata.len() < 512 {
        return Err(invalid("empty or unsupported base database"));
    }
    let initial = Stamp::of(&metadata);
    let mut header = [0; 100];
    read_at(&base, &mut header, 0)?;
    let (page_size, usable) = page_geometry(&header)?;
    if metadata.len() % page_size as u64 != 0 {
        return Err(invalid("truncated base database page"));
    }
    let physical_pages = u32::try_from(metadata.len() / page_size as u64)
        .map_err(|_| invalid("base database too large"))?;
    if physical_pages == 0 || physical_pages > MAX_PAGE_NUMBER {
        return Err(invalid("unsupported base database size"));
    }
    let header_pages = be32(&header, 28)?;
    let base_pages = if header_pages != 0 && be32(&header, 24)? == be32(&header, 92)? {
        if header_pages > physical_pages {
            return Err(invalid("truncated declared base database"));
        }
        header_pages
    } else {
        physical_pages
    };
    let wal = Wal::open(copied_wal, page_size)?;
    let database_pages = wal
        .as_ref()
        .and_then(|wal| wal.database_pages)
        .unwrap_or(base_pages);
    let staged = OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .mode(0o600)
        .open(staged)
        .map_err(Error::Io)?;
    staged.set_len(initial.len).map_err(Error::Io)?;
    let mut snapshot = SchemaSnapshot {
        base,
        staged,
        initial,
        wal,
        page_size,
        usable,
        physical_pages,
        database_pages,
        limit: MAX_PAGES.min(MAX_BYTES / page_size),
        originals: BTreeMap::new(),
        visited: BTreeSet::new(),
        roots: BTreeSet::new(),
        poisoned: false,
    };
    snapshot.copy_tree(1)?;
    Ok(snapshot)
}

impl SchemaSnapshot {
    fn claim(&mut self, number: u32) -> Result<()> {
        if number == 0 || number > self.database_pages {
            return Err(invalid("page reference outside committed database"));
        }
        if !self.visited.insert(number) {
            return Err(invalid("cyclic or shared metadata page reference"));
        }
        if self.visited.len() > self.limit {
            return Err(invalid("metadata page/byte limit exceeded"));
        }
        Ok(())
    }

    fn page(&mut self, number: u32) -> Result<Vec<u8>> {
        let offset = u64::from(number - 1) * self.page_size as u64;
        if number <= self.physical_pages && !self.originals.contains_key(&number) {
            let mut original = vec![0; self.page_size];
            read_at(&self.base, &mut original, offset)?;
            self.staged
                .write_all_at(&original, offset)
                .map_err(Error::Io)?;
            self.originals.insert(number, original);
        }
        if let Some(wal) = &self.wal
            && let Some(wal_offset) = wal.pages.get(&number)
        {
            let mut effective = vec![0; self.page_size];
            read_at(&wal.file, &mut effective, *wal_offset)?;
            return Ok(effective);
        }
        self.originals
            .get(&number)
            .cloned()
            .ok_or_else(|| invalid("missing committed WAL page beyond base"))
    }

    /// Repeated calls for a completed root are idempotent. Any failure poisons
    /// the candidate; callers must abandon it instead of continuing partially.
    pub(super) fn copy_tree(&mut self, root: u32) -> Result<()> {
        if self.poisoned {
            return Err(invalid("candidate already failed"));
        }
        if self.roots.contains(&root) {
            return Ok(());
        }
        let result = self.walk(root);
        if result.is_err() {
            self.poisoned = true;
        } else {
            self.roots.insert(root);
        }
        result
    }

    fn walk(&mut self, root: u32) -> Result<()> {
        let mut pending = vec![(root, None)];
        while let Some((number, expected)) = pending.pop() {
            self.claim(number)?;
            let page = self.page(number)?;
            let header = if number == 1 {
                if page_geometry(&page[..100])? != (self.page_size, self.usable) {
                    return Err(invalid("WAL changed database page geometry"));
                }
                100
            } else {
                0
            };
            let kind = page[header];
            let family = match kind {
                5 | 13 => Family::Table,
                2 | 10 => Family::Index,
                _ => return Err(invalid("unsupported btree page type")),
            };
            if (number == 1 && family != Family::Table)
                || expected.is_some_and(|expected| expected != family)
            {
                return Err(invalid("inconsistent btree page family"));
            }
            let interior = matches!(kind, 2 | 5);
            let cells = usize::from(be16(&page, header + 3)?);
            let array_start = header + if interior { 12 } else { 8 };
            let array_end = array_start + cells * 2;
            let content = usize::from(be16(&page, header + 5)?);
            let content = if content == 0 { 65536 } else { content };
            let free = usize::from(be16(&page, header + 1)?);
            if array_end > self.usable
                || content < array_end
                || content > self.usable
                || page[header + 7] > 60
                || (free != 0 && (free < array_end || free + 4 > self.usable))
            {
                return Err(invalid("invalid btree header offsets"));
            }
            if interior {
                pending.push((be32(&page, header + 8)?, Some(family)));
            }
            let mut offsets = BTreeSet::new();
            for cell in 0..cells {
                let mut position = usize::from(be16(&page, array_start + cell * 2)?);
                if position < content || position >= self.usable || !offsets.insert(position) {
                    return Err(invalid("invalid or duplicate cell pointer"));
                }
                if interior {
                    if position + 4 > self.usable {
                        return Err(invalid("truncated interior cell"));
                    }
                    pending.push((be32(&page, position)?, Some(family)));
                    position += 4;
                }
                if kind == 5 {
                    varint(&page[..self.usable], &mut position)?;
                    continue;
                }
                let payload = varint(&page[..self.usable], &mut position)?;
                if kind == 13 {
                    varint(&page[..self.usable], &mut position)?;
                }
                let usable = self.usable as u64;
                let minimum = (usable - 12) * 32 / 255 - 23;
                let maximum = if kind == 13 {
                    usable - 35
                } else {
                    (usable - 12) * 64 / 255 - 23
                };
                let local = if payload <= maximum {
                    payload
                } else {
                    let candidate = minimum + (payload - minimum) % (usable - 4);
                    if candidate <= maximum {
                        candidate
                    } else {
                        minimum
                    }
                };
                // local is bounded by maximum, hence by a single usable page.
                let end = position + local as usize;
                if end > self.usable {
                    return Err(invalid("truncated local cell payload"));
                }
                if local < payload {
                    if end + 4 > self.usable {
                        return Err(invalid("truncated overflow pointer"));
                    }
                    self.overflow(be32(&page, end)?, payload - local)?;
                }
            }
            // Bound queued references as well as pages actually visited.
            if pending.len() > self.limit {
                return Err(invalid("btree work queue exceeds metadata limit"));
            }
        }
        Ok(())
    }

    fn overflow(&mut self, mut number: u32, mut remaining: u64) -> Result<()> {
        let capacity = (self.usable - 4) as u64;
        while remaining != 0 {
            self.claim(number)?;
            let page = self.page(number)?;
            let next = be32(&page, 0)?;
            remaining = remaining.saturating_sub(capacity);
            if (remaining == 0) != (next == 0) {
                return Err(invalid("overflow chain length disagrees with payload"));
            }
            number = next;
        }
        Ok(())
    }

    /// Does not hash/read unrelated base pages. The caller must still check
    /// original path identity and WAL/journal stability around the operation.
    pub(super) fn finish(self) -> Result<()> {
        if self.poisoned {
            return Err(invalid("candidate already failed"));
        }
        if Stamp::of(&self.base.metadata().map_err(Error::Io)?) != self.initial {
            return Err(invalid("base metadata changed during snapshot"));
        }
        let mut current = vec![0; self.page_size];
        for (number, original) in &self.originals {
            read_at(
                &self.base,
                &mut current,
                u64::from(*number - 1) * self.page_size as u64,
            )?;
            if current != *original {
                return Err(invalid("copied base page changed during snapshot"));
            }
        }
        if Stamp::of(&self.base.metadata().map_err(Error::Io)?) != self.initial {
            return Err(invalid("base metadata changed during page verification"));
        }
        if let Some(wal) = &self.wal
            && Stamp::of(&wal.file.metadata().map_err(Error::Io)?) != wal.stamp
        {
            return Err(invalid("private WAL changed during snapshot"));
        }
        Ok(())
    }
}

fn varint(page: &[u8], position: &mut usize) -> Result<u64> {
    let mut value = 0_u64;
    for index in 0..9 {
        let byte = *page
            .get(*position)
            .ok_or_else(|| invalid("truncated cell varint"))?;
        *position += 1;
        if index == 8 {
            return Ok((value << 8) | u64::from(byte));
        }
        value = (value << 7) | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(invalid("invalid cell varint"))
}
