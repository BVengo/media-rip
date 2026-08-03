use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::Path,
};

use media_rip_core::MediaEngineError;

use super::{
    ProbeCandidate, ProbeInput, locate_child_case_insensitive, normalize_disc_root, source_stem,
};

const DVD_SECTOR_SIZE: usize = 2048;
const TT_SRPT_SECTOR_OFFSET: usize = 0xC4;
const VTS_PTT_SRPT_SECTOR_OFFSET: usize = 0xC8;
const VTS_PGCIT_SECTOR_OFFSET: usize = 0xCC;
const TXTDT_MGI_SECTOR_OFFSET: usize = 0xD4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DvdTitleReference {
    title_set: u8,
    title_number: u8,
}

pub(crate) fn dvd_candidates(path: &Path) -> Result<Vec<ProbeCandidate>, MediaEngineError> {
    let root = normalize_disc_root(path, "VIDEO_TS");
    if !root.is_dir() {
        let title_references = read_title_references_from_dvd(&root).unwrap_or_default();
        let title_count = (!title_references.is_empty())
            .then_some(title_references.len() as u32)
            .or_else(|| read_title_count_from_dvd(&root))
            .ok_or_else(|| {
                MediaEngineError::UnsupportedInput(format!(
                    "could not read the DVD-Video title table from {}",
                    root.display()
                ))
            })?;
        let size_hints = title_sizes_from_dvd(&root, &title_references);
        return Ok(title_candidates(root, title_count, &size_hints));
    }
    let video_ts =
        locate_child_case_insensitive(&root, "VIDEO_TS").unwrap_or_else(|| root.join("VIDEO_TS"));
    let manager = locate_child_case_insensitive(&video_ts, "VIDEO_TS.IFO")
        .unwrap_or_else(|| video_ts.join("VIDEO_TS.IFO"));

    let title_references = read_title_references(&manager).unwrap_or_default();
    let title_count = (!title_references.is_empty())
        .then_some(title_references.len() as u32)
        .or_else(|| read_title_count(&manager))
        .or_else(|| read_title_count_from_dvd(&root))
        .unwrap_or_else(|| count_title_sets(&video_ts));
    if title_count == 0 {
        return Err(MediaEngineError::UnsupportedInput(format!(
            "{} does not contain a readable DVD-Video title table",
            root.display()
        )));
    }

    let size_hints = title_sizes_from_directory(&video_ts, &title_references);
    Ok(title_candidates(root, title_count, &size_hints))
}

pub(crate) fn dvd_text_name(path: &Path) -> Option<String> {
    let root = normalize_disc_root(path, "VIDEO_TS");
    let bytes = if root.is_dir() {
        let video_ts = locate_child_case_insensitive(&root, "VIDEO_TS")
            .unwrap_or_else(|| root.join("VIDEO_TS"));
        let manager = locate_child_case_insensitive(&video_ts, "VIDEO_TS.IFO")
            .unwrap_or_else(|| video_ts.join("VIDEO_TS.IFO"));
        fs::read(manager).ok()?
    } else {
        #[cfg(target_os = "linux")]
        {
            read_info_file_from_dvd(&root, 0)?
        }
        #[cfg(not(target_os = "linux"))]
        {
            return None;
        }
    };
    dvd_text_name_from_manager(&bytes)
}

fn dvd_text_name_from_manager(bytes: &[u8]) -> Option<String> {
    let end = TXTDT_MGI_SECTOR_OFFSET.checked_add(4)?;
    let sector = read_be_u32(bytes.get(TXTDT_MGI_SECTOR_OFFSET..end)?)?;
    if sector == 0 {
        return None;
    }
    let offset = usize::try_from(sector).ok()?.checked_mul(DVD_SECTOR_SIZE)?;
    let raw = bytes.get(offset..offset.checked_add(12)?)?;
    let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    let name = String::from_utf8_lossy(&raw[..end]).trim().to_owned();
    (!name.is_empty() && name.chars().all(|character| !character.is_control())).then_some(name)
}

fn title_candidates(
    root: std::path::PathBuf,
    title_count: u32,
    size_hints: &[Option<u64>],
) -> Vec<ProbeCandidate> {
    let disc_name = source_stem(&root);
    (1..=title_count)
        .enumerate()
        .map(|(index, title)| ProbeCandidate {
            id: title,
            source_title_id: title.to_string(),
            disc_title_number: Some(title),
            display_name: format!("{disc_name} — Title {title}"),
            input: ProbeInput::Dvd {
                root: root.clone(),
                title,
            },
            size_hint: size_hints.get(index).copied().flatten(),
            size_hint_is_estimate: false,
            segment_count: None,
            segment_map: None,
            chapters: None,
            duplicate_key: None,
            stream_languages: Default::default(),
            authored_stream_pids: None,
            is_main_title: false,
        })
        .collect()
}

fn read_title_count(path: &Path) -> Option<u32> {
    let bytes = fs::read(path).ok()?;
    title_count_from_manager(&bytes)
}

fn read_title_references(path: &Path) -> Option<Vec<DvdTitleReference>> {
    let bytes = fs::read(path).ok()?;
    read_title_references_from_manager(&bytes)
}

fn title_count_from_manager(bytes: &[u8]) -> Option<u32> {
    let table = title_table_from_manager(bytes)?;
    read_be_u16(table.get(..2)?)
        .filter(|count| *count > 0)
        .map(u32::from)
}

fn read_title_references_from_manager(bytes: &[u8]) -> Option<Vec<DvdTitleReference>> {
    read_title_references_from_table(title_table_from_manager(bytes)?)
}

fn title_table_from_manager(bytes: &[u8]) -> Option<&[u8]> {
    let sector = read_be_u32(bytes.get(TT_SRPT_SECTOR_OFFSET..TT_SRPT_SECTOR_OFFSET + 4)?)?;
    bytes.get((sector as usize).checked_mul(DVD_SECTOR_SIZE)?..)
}

fn read_title_references_from_table(table: &[u8]) -> Option<Vec<DvdTitleReference>> {
    let count = read_be_u16(table.get(..2)?)? as usize;
    (0..count)
        .map(|index| {
            let offset = 8_usize.checked_add(index.checked_mul(12)?)?;
            let title_set = table.get(offset + 6).copied()?;
            let title_number = table.get(offset + 7).copied()?;
            (title_set > 0 && title_number > 0).then_some(DvdTitleReference {
                title_set,
                title_number,
            })
        })
        .collect()
}

fn title_sizes_from_directory(video_ts: &Path, titles: &[DvdTitleReference]) -> Vec<Option<u64>> {
    let mut title_sets = HashMap::new();
    titles
        .iter()
        .map(|title| {
            let bytes = title_sets.entry(title.title_set).or_insert_with(|| {
                locate_child_case_insensitive(
                    video_ts,
                    &format!("VTS_{:02}_0.IFO", title.title_set),
                )
                .and_then(|path| fs::read(path).ok())
            });
            bytes
                .as_deref()
                .and_then(|bytes| title_size_from_vts_ifo(bytes, title.title_number))
        })
        .collect()
}

fn title_size_from_vts_ifo(bytes: &[u8], title_number: u8) -> Option<u64> {
    let programs = title_programs(bytes, title_number)?;
    let pgcit = table_at_sector_pointer(bytes, VTS_PGCIT_SECTOR_OFFSET)?;
    let pgc_count = read_be_u16(pgcit.get(..2)?)?;
    let mut cells = BTreeSet::new();

    for (pgcn, pgn) in programs {
        if pgcn == 0 || pgcn > pgc_count {
            return None;
        }
        let search_pointer = 8_usize.checked_add((usize::from(pgcn) - 1).checked_mul(8)?)?;
        let pgc_offset = read_be_u32(pgcit.get(search_pointer + 4..search_pointer + 8)?)? as usize;
        let pgc = pgcit.get(pgc_offset..)?;
        let program_count = usize::from(*pgc.get(2)?);
        let cell_count = usize::from(*pgc.get(3)?);
        let program = usize::from(pgn);
        if program == 0 || program > program_count || cell_count == 0 {
            return None;
        }
        let program_map = read_be_u16(pgc.get(230..232)?)? as usize;
        let cell_playback = read_be_u16(pgc.get(232..234)?)? as usize;
        let first_cell = usize::from(*pgc.get(program_map + program - 1)?);
        let next_cell = if program < program_count {
            usize::from(*pgc.get(program_map + program)?)
        } else {
            cell_count + 1
        };
        if first_cell == 0 || first_cell >= next_cell || next_cell > cell_count + 1 {
            return None;
        }
        for cell in first_cell..next_cell {
            cells.insert((pgcn, pgc_offset, cell_playback, cell));
        }
    }

    cells
        .into_iter()
        .try_fold(0_u64, |total, (_pgcn, pgc_offset, cell_playback, cell)| {
            let pgc = pgcit.get(pgc_offset..)?;
            let cell_offset = cell_playback.checked_add((cell - 1).checked_mul(24)?)?;
            let cell = pgc.get(cell_offset..cell_offset + 24)?;
            let block_mode = cell[0] >> 6;
            let block_type = (cell[0] >> 4) & 0x03;
            if block_type == 1 && block_mode != 1 {
                return Some(total);
            }
            let first_sector = read_be_u32(cell.get(8..12)?)?;
            let last_sector = read_be_u32(cell.get(20..24)?)?;
            let sectors = last_sector.checked_sub(first_sector)?.checked_add(1)?;
            total.checked_add(u64::from(sectors).checked_mul(DVD_SECTOR_SIZE as u64)?)
        })
}

fn title_programs(bytes: &[u8], title_number: u8) -> Option<BTreeSet<(u16, u16)>> {
    let table = table_at_sector_pointer(bytes, VTS_PTT_SRPT_SECTOR_OFFSET)?;
    let title_count = usize::from(read_be_u16(table.get(..2)?)?);
    let title_index = usize::from(title_number).checked_sub(1)?;
    if title_index >= title_count {
        return None;
    }
    let offset_entry = 8_usize.checked_add(title_index.checked_mul(4)?)?;
    let start = read_be_u32(table.get(offset_entry..offset_entry + 4)?)? as usize;
    let end = if title_index + 1 < title_count {
        let next_entry = offset_entry.checked_add(4)?;
        read_be_u32(table.get(next_entry..next_entry + 4)?)? as usize
    } else {
        (read_be_u32(table.get(4..8)?)? as usize).checked_add(1)?
    };
    if start >= end || end > table.len() || (end - start) % 4 != 0 {
        return None;
    }
    let programs = table[start..end]
        .chunks_exact(4)
        .filter_map(|entry| {
            let pgcn = read_be_u16(entry.get(..2)?)?;
            let pgn = read_be_u16(entry.get(2..4)?)?;
            (pgcn > 0 && pgn > 0).then_some((pgcn, pgn))
        })
        .collect::<BTreeSet<_>>();
    (!programs.is_empty()).then_some(programs)
}

fn table_at_sector_pointer(bytes: &[u8], pointer: usize) -> Option<&[u8]> {
    let sector = read_be_u32(bytes.get(pointer..pointer + 4)?)? as usize;
    bytes.get(sector.checked_mul(DVD_SECTOR_SIZE)?..)
}

#[cfg(target_os = "linux")]
fn read_title_count_from_dvd(path: &Path) -> Option<u32> {
    title_count_from_manager(&read_info_file_from_dvd(path, 0)?)
}

#[cfg(not(target_os = "linux"))]
fn read_title_count_from_dvd(_path: &Path) -> Option<u32> {
    None
}

#[cfg(target_os = "linux")]
const DVD_READ_INFO_FILE: i32 = 0;

#[cfg(target_os = "linux")]
fn read_title_references_from_dvd(path: &Path) -> Option<Vec<DvdTitleReference>> {
    read_title_references_from_manager(&read_info_file_from_dvd(path, 0)?)
}

#[cfg(not(target_os = "linux"))]
fn read_title_references_from_dvd(_path: &Path) -> Option<Vec<DvdTitleReference>> {
    None
}

#[cfg(target_os = "linux")]
fn title_sizes_from_dvd(path: &Path, titles: &[DvdTitleReference]) -> Vec<Option<u64>> {
    let mut title_sets = HashMap::new();
    titles
        .iter()
        .map(|title| {
            let bytes = title_sets
                .entry(title.title_set)
                .or_insert_with(|| read_info_file_from_dvd(path, title.title_set));
            bytes
                .as_deref()
                .and_then(|bytes| title_size_from_vts_ifo(bytes, title.title_number))
        })
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn title_sizes_from_dvd(_path: &Path, titles: &[DvdTitleReference]) -> Vec<Option<u64>> {
    vec![None; titles.len()]
}

#[cfg(target_os = "linux")]
fn read_info_file_from_dvd(path: &Path, title_set: u8) -> Option<Vec<u8>> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let path = CString::new(path.as_os_str().as_bytes()).ok()?;

    // SAFETY: The checked libdvdread handles are closed on all paths. The
    // destination owns enough space for the block count returned by the same
    // file handle, and DVDReadBytes is passed that exact capacity.
    unsafe {
        let dvd = dvd_open(path.as_ptr());
        if dvd.is_null() {
            return None;
        }
        let file = dvd_open_file(dvd, i32::from(title_set), DVD_READ_INFO_FILE);
        if file.is_null() {
            dvd_close(dvd);
            return None;
        }
        let result = (|| {
            let blocks = usize::try_from(dvd_file_size(file)).ok()?;
            let capacity = blocks.checked_mul(DVD_SECTOR_SIZE)?;
            let mut bytes = vec![0_u8; capacity];
            let bytes_read =
                usize::try_from(dvd_read_bytes(file, bytes.as_mut_ptr().cast(), capacity)).ok()?;
            (bytes_read > 0 && bytes_read <= capacity).then(|| {
                bytes.truncate(bytes_read);
                bytes
            })
        })();
        dvd_close_file(file);
        dvd_close(dvd);
        result
    }
}

#[cfg(target_os = "linux")]
#[link(name = "dvdread")]
unsafe extern "C" {
    #[link_name = "DVDOpen"]
    fn dvd_open(path: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    #[link_name = "DVDClose"]
    fn dvd_close(dvd: *mut std::ffi::c_void);
    #[link_name = "DVDOpenFile"]
    fn dvd_open_file(dvd: *mut std::ffi::c_void, title: i32, domain: i32) -> *mut std::ffi::c_void;
    #[link_name = "DVDCloseFile"]
    fn dvd_close_file(file: *mut std::ffi::c_void);
    #[link_name = "DVDFileSize"]
    fn dvd_file_size(file: *mut std::ffi::c_void) -> isize;
    #[link_name = "DVDReadBytes"]
    fn dvd_read_bytes(
        file: *mut std::ffi::c_void,
        data: *mut std::ffi::c_void,
        byte_count: usize,
    ) -> isize;
}

fn count_title_sets(video_ts: &Path) -> u32 {
    fs::read_dir(video_ts)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_ascii_uppercase))
        .filter(|name| name.starts_with("VTS_") && name.ends_with("_0.IFO") && name.len() == 12)
        .count() as u32
}

fn read_be_u16(bytes: &[u8]) -> Option<u16> {
    let bytes: [u8; 2] = bytes.try_into().ok()?;
    Some(u16::from_be_bytes(bytes))
}

fn read_be_u32(bytes: &[u8]) -> Option<u32> {
    let bytes: [u8; 4] = bytes.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_optional_dvd_text_disc_name() {
        let mut bytes = vec![0_u8; DVD_SECTOR_SIZE * 2];
        bytes[TXTDT_MGI_SECTOR_OFFSET..TXTDT_MGI_SECTOR_OFFSET + 4]
            .copy_from_slice(&1_u32.to_be_bytes());
        bytes[DVD_SECTOR_SIZE..DVD_SECTOR_SIZE + 11].copy_from_slice(b"Example DVD");

        assert_eq!(
            dvd_text_name_from_manager(&bytes).as_deref(),
            Some("Example DVD")
        );
    }

    #[test]
    fn reads_title_count_from_video_manager_table() {
        let mut bytes = vec![0; DVD_SECTOR_SIZE * 2];
        bytes[TT_SRPT_SECTOR_OFFSET..TT_SRPT_SECTOR_OFFSET + 4]
            .copy_from_slice(&1_u32.to_be_bytes());
        bytes[DVD_SECTOR_SIZE..DVD_SECTOR_SIZE + 2].copy_from_slice(&7_u16.to_be_bytes());

        let path = std::env::temp_dir().join(format!(
            "media-rip-dvd-ifo-test-{}-{}.ifo",
            std::process::id(),
            std::thread::current().name().unwrap_or("thread")
        ));
        fs::write(&path, bytes).unwrap();
        assert_eq!(read_title_count(&path), Some(7));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn creates_one_probe_candidate_for_each_dvd_title() {
        let candidates = title_candidates("/disc".into(), 3, &[]);

        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0].source_title_id, "1");
        assert_eq!(candidates[2].source_title_id, "3");
        assert!(matches!(
            candidates[1].input,
            ProbeInput::Dvd { title: 2, .. }
        ));
    }

    #[test]
    fn reads_title_references_from_the_video_manager_table() {
        let mut bytes = vec![0; DVD_SECTOR_SIZE * 2];
        bytes[TT_SRPT_SECTOR_OFFSET..TT_SRPT_SECTOR_OFFSET + 4]
            .copy_from_slice(&1_u32.to_be_bytes());
        bytes[DVD_SECTOR_SIZE..DVD_SECTOR_SIZE + 2].copy_from_slice(&2_u16.to_be_bytes());
        bytes[DVD_SECTOR_SIZE + 8 + 6] = 1;
        bytes[DVD_SECTOR_SIZE + 8 + 7] = 2;
        bytes[DVD_SECTOR_SIZE + 20 + 6] = 3;
        bytes[DVD_SECTOR_SIZE + 20 + 7] = 4;

        let path = std::env::temp_dir().join(format!(
            "media-rip-dvd-title-set-test-{}.ifo",
            std::process::id()
        ));
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            read_title_references(&path),
            Some(vec![
                DvdTitleReference {
                    title_set: 1,
                    title_number: 2
                },
                DvdTitleReference {
                    title_set: 3,
                    title_number: 4
                }
            ])
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn calculates_a_title_size_from_its_program_cells() {
        let mut bytes = vec![0_u8; 4_600];
        bytes[VTS_PTT_SRPT_SECTOR_OFFSET..VTS_PTT_SRPT_SECTOR_OFFSET + 4]
            .copy_from_slice(&1_u32.to_be_bytes());
        bytes[VTS_PGCIT_SECTOR_OFFSET..VTS_PGCIT_SECTOR_OFFSET + 4]
            .copy_from_slice(&2_u32.to_be_bytes());

        let ptt = DVD_SECTOR_SIZE;
        bytes[ptt..ptt + 2].copy_from_slice(&1_u16.to_be_bytes());
        bytes[ptt + 4..ptt + 8].copy_from_slice(&15_u32.to_be_bytes());
        bytes[ptt + 8..ptt + 12].copy_from_slice(&12_u32.to_be_bytes());
        bytes[ptt + 12..ptt + 14].copy_from_slice(&1_u16.to_be_bytes());
        bytes[ptt + 14..ptt + 16].copy_from_slice(&1_u16.to_be_bytes());

        let pgcit = DVD_SECTOR_SIZE * 2;
        bytes[pgcit..pgcit + 2].copy_from_slice(&1_u16.to_be_bytes());
        bytes[pgcit + 12..pgcit + 16].copy_from_slice(&16_u32.to_be_bytes());
        let pgc = pgcit + 16;
        bytes[pgc + 2] = 1;
        bytes[pgc + 3] = 2;
        bytes[pgc + 230..pgc + 232].copy_from_slice(&236_u16.to_be_bytes());
        bytes[pgc + 232..pgc + 234].copy_from_slice(&237_u16.to_be_bytes());
        bytes[pgc + 236] = 1;

        let first_cell = pgc + 237;
        bytes[first_cell + 8..first_cell + 12].copy_from_slice(&100_u32.to_be_bytes());
        bytes[first_cell + 20..first_cell + 24].copy_from_slice(&199_u32.to_be_bytes());
        let second_cell = first_cell + 24;
        bytes[second_cell + 8..second_cell + 12].copy_from_slice(&300_u32.to_be_bytes());
        bytes[second_cell + 20..second_cell + 24].copy_from_slice(&349_u32.to_be_bytes());

        assert_eq!(
            title_size_from_vts_ifo(&bytes, 1),
            Some(150 * DVD_SECTOR_SIZE as u64)
        );
    }
}
