mod layout;

use std::{collections::HashMap, fs, path::Path};

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
    let load = |title_set: u8, backup: bool| {
        if root.is_dir() {
            let video_ts = locate_child_case_insensitive(&root, "VIDEO_TS")
                .unwrap_or_else(|| root.join("VIDEO_TS"));
            let stem = if title_set == 0 {
                "VIDEO_TS".to_owned()
            } else {
                format!("VTS_{title_set:02}_0")
            };
            let extension = if backup { "BUP" } else { "IFO" };
            let path = locate_child_case_insensitive(&video_ts, &format!("{stem}.{extension}"))?;
            let size = fs::metadata(&path).ok()?.len();
            if size > 16 * 1024 * 1024 {
                return None;
            }
            fs::read(path).ok()
        } else {
            read_info_file_from_dvd(&root, title_set, backup)
        }
    };
    let references = load(0, false)
        .and_then(|bytes| read_title_references_from_manager(&bytes))
        .or_else(|| load(0, true).and_then(|bytes| read_title_references_from_manager(&bytes)))
        .ok_or_else(|| {
            MediaEngineError::UnsupportedInput(format!(
                "No valid DVD title table in IFO or BUP at {}",
                root.display()
            ))
        })?;
    let mut candidates = title_candidates(root.clone(), references.len() as u32, &[]);
    let mut title_sets = HashMap::new();
    for (candidate, (disc_title, reference)) in candidates.iter_mut().zip(references) {
        candidate.id = disc_title;
        candidate.source_title_id = disc_title.to_string();
        candidate.disc_title_number = Some(disc_title);
        if let ProbeInput::Dvd { title, .. } = &mut candidate.input {
            *title = disc_title;
        }
        let (primary, backup, vob_sectors) =
            title_sets.entry(reference.title_set).or_insert_with(|| {
                (
                    load(reference.title_set, false),
                    load(reference.title_set, true),
                    title_vob_sectors(&root, reference.title_set),
                )
            });
        let primary_layout = primary
            .as_deref()
            .ok_or_else(|| "Title-set IFO is unreadable".to_owned())
            .and_then(|bytes| layout::analyse(bytes, reference, *vob_sectors));
        let analysed = primary_layout.or_else(|primary_error| {
            backup
                .as_deref()
                .and_then(|bytes| layout::analyse(bytes, reference, *vob_sectors).ok())
                .ok_or(primary_error)
        });
        let info = match analysed {
            Ok(layout) => {
                candidate.size_hint = Some(layout.size);
                candidate.size_hint_is_estimate = true;
                candidate.segment_count = Some(layout.info.cell_count);
                candidate.segment_map = Some(layout.map);
                candidate.chapters = Some(layout.chapters);
                candidate.duplicate_key = Some(layout.duplicate_key);
                layout.info
            }
            Err(error) => media_rip_core::model::dvd::DvdTitleInfo {
                title_set: reference.title_set,
                title_in_set: reference.title_number,
                chapter_range: None,
                cell_count: 0,
                leading_padding_cells: 0,
                trailing_padding_cells: 0,
                warnings: vec![format!("DVD layout could not be validated: {error}")],
            },
        };
        candidate.display_name = format!(
            "DVD title {} · VTS {:02}, title {}",
            candidate.source_title_id, reference.title_set, reference.title_number
        );
        if let ProbeInput::Dvd { info: slot, .. } = &mut candidate.input {
            *slot = Some(info);
        }
    }
    Ok(candidates)
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
            read_info_file_from_dvd(&root, 0, false)?
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
                info: None,
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

fn read_title_references_from_manager(bytes: &[u8]) -> Option<Vec<(u32, DvdTitleReference)>> {
    read_title_references_from_table(title_table_from_manager(bytes)?)
}

fn title_table_from_manager(bytes: &[u8]) -> Option<&[u8]> {
    let sector = read_be_u32(bytes.get(TT_SRPT_SECTOR_OFFSET..TT_SRPT_SECTOR_OFFSET + 4)?)?;
    if sector == 0 {
        return None;
    }
    bytes.get((sector as usize).checked_mul(DVD_SECTOR_SIZE)?..)
}

fn read_title_references_from_table(table: &[u8]) -> Option<Vec<(u32, DvdTitleReference)>> {
    let count = read_be_u16(table.get(..2)?)? as usize;
    if count == 0 || count > 99 || table.len() < 8 + count * 12 {
        return None;
    }
    let references = (0..count)
        .filter_map(|index| {
            let offset = 8 + index * 12;
            let title_set = table[offset + 6];
            let title_number = table[offset + 7];
            (title_set > 0 && title_set <= 99 && title_number > 0 && title_number <= 99).then_some(
                (
                    index as u32 + 1,
                    DvdTitleReference {
                        title_set,
                        title_number,
                    },
                ),
            )
        })
        .collect::<Vec<_>>();
    (!references.is_empty()).then_some(references)
}

fn table_at_sector_pointer(bytes: &[u8], pointer: usize) -> Option<&[u8]> {
    let sector = read_be_u32(bytes.get(pointer..pointer + 4)?)? as usize;
    if sector == 0 {
        return None;
    }
    bytes.get(sector.checked_mul(DVD_SECTOR_SIZE)?..)
}

fn title_vob_sectors(root: &Path, title_set: u8) -> Option<u64> {
    if root.is_dir() {
        let video_ts = locate_child_case_insensitive(root, "VIDEO_TS")
            .unwrap_or_else(|| root.join("VIDEO_TS"));
        let mut bytes = 0_u64;
        for part in 1..=9 {
            let Some(path) =
                locate_child_case_insensitive(&video_ts, &format!("VTS_{title_set:02}_{part}.VOB"))
            else {
                break;
            };
            bytes = bytes.checked_add(fs::metadata(path).ok()?.len())?;
        }
        return (bytes > 0 && bytes.is_multiple_of(DVD_SECTOR_SIZE as u64))
            .then_some(bytes / DVD_SECTOR_SIZE as u64);
    }
    #[cfg(target_os = "linux")]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let path = CString::new(root.as_os_str().as_bytes()).ok()?;
        // SAFETY: Handles are checked and closed. DVDFileSize reports sectors
        // for the concatenated title VOBs (libdvdread domain 3).
        unsafe {
            let dvd = dvd_open(path.as_ptr());
            if dvd.is_null() {
                return None;
            }
            let file = dvd_open_file(dvd, i32::from(title_set), 3);
            if file.is_null() {
                dvd_close(dvd);
                return None;
            }
            let sectors = u64::try_from(dvd_file_size(file))
                .ok()
                .filter(|size| *size > 0);
            dvd_close_file(file);
            dvd_close(dvd);
            sectors
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn read_info_file_from_dvd(path: &Path, title_set: u8, backup: bool) -> Option<Vec<u8>> {
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
        let file = dvd_open_file(dvd, i32::from(title_set), i32::from(backup));
        if file.is_null() {
            dvd_close(dvd);
            return None;
        }
        let result = (|| {
            let blocks = usize::try_from(dvd_file_size(file)).ok()?;
            let capacity = blocks.checked_mul(DVD_SECTOR_SIZE)?;
            if capacity == 0 || capacity > 16 * 1024 * 1024 {
                return None;
            }
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

fn read_be_u16(bytes: &[u8]) -> Option<u16> {
    let bytes: [u8; 2] = bytes.try_into().ok()?;
    Some(u16::from_be_bytes(bytes))
}

fn read_be_u32(bytes: &[u8]) -> Option<u32> {
    let bytes: [u8; 4] = bytes.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

#[cfg(not(target_os = "linux"))]
fn read_info_file_from_dvd(_path: &Path, _title_set: u8, _backup: bool) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_invalid_references_without_renumbering_remaining_titles() {
        let mut table = vec![0; 8 + 99 * 12];
        table[..2].copy_from_slice(&99_u16.to_be_bytes());
        table[8 + 64 * 12 + 6] = 1;
        table[8 + 64 * 12 + 7] = 1;
        let references = read_title_references_from_table(&table).unwrap();
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].0, 65);
    }

    #[test]
    fn uses_valid_backup_tables_when_ifo_files_are_unreadable() {
        let root =
            std::env::temp_dir().join(format!("media-rip-dvd-backup-{}", std::process::id()));
        let video_ts = root.join("VIDEO_TS");
        fs::create_dir_all(&video_ts).unwrap();
        let mut manager = vec![0; DVD_SECTOR_SIZE * 2];
        manager[..12].copy_from_slice(b"DVDVIDEO-VMG");
        manager[TT_SRPT_SECTOR_OFFSET..TT_SRPT_SECTOR_OFFSET + 4]
            .copy_from_slice(&1_u32.to_be_bytes());
        manager[DVD_SECTOR_SIZE..DVD_SECTOR_SIZE + 2].copy_from_slice(&1_u16.to_be_bytes());
        manager[DVD_SECTOR_SIZE + 14] = 1;
        manager[DVD_SECTOR_SIZE + 15] = 1;
        fs::write(video_ts.join("VIDEO_TS.IFO"), [0; 16]).unwrap();
        fs::write(video_ts.join("VIDEO_TS.BUP"), manager).unwrap();
        fs::write(video_ts.join("VTS_01_0.IFO"), [0; 16]).unwrap();
        fs::write(
            video_ts.join("VTS_01_0.BUP"),
            layout::tests::fixture(&[(60_000, 0, 99)]),
        )
        .unwrap();
        fs::File::create(video_ts.join("VTS_01_1.VOB"))
            .unwrap()
            .set_len(100 * 2048)
            .unwrap();
        let candidates = dvd_candidates(&root).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].size_hint, Some(100 * 2048));
        let ProbeInput::Dvd {
            info: Some(info), ..
        } = &candidates[0].input
        else {
            panic!("missing DVD analysis")
        };
        assert!(info.warnings.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_impossible_title_counts_and_truncated_tables() {
        for count in [0_u16, 100, u16::MAX] {
            let mut table = vec![0; 1200];
            table[..2].copy_from_slice(&count.to_be_bytes());
            assert!(read_title_references_from_table(&table).is_none());
        }
        assert!(read_title_references_from_table(&[0, 2, 0, 0, 0, 0, 0, 0]).is_none());
    }

    #[test]
    fn keeps_authored_title_numbers_in_probe_candidates() {
        let candidates = title_candidates("/disc".into(), 99, &[]);
        assert_eq!(candidates[64].disc_title_number, Some(65));
        assert_eq!(candidates[64].source_title_id, "65");
    }

    #[test]
    fn reads_optional_disc_text_name() {
        let mut bytes = vec![0; DVD_SECTOR_SIZE * 2];
        bytes[TXTDT_MGI_SECTOR_OFFSET..TXTDT_MGI_SECTOR_OFFSET + 4]
            .copy_from_slice(&1_u32.to_be_bytes());
        bytes[DVD_SECTOR_SIZE..DVD_SECTOR_SIZE + 11].copy_from_slice(b"Example DVD");
        assert_eq!(
            dvd_text_name_from_manager(&bytes).as_deref(),
            Some("Example DVD")
        );
    }
}
