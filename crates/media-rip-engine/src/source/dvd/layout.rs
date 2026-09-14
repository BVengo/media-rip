//! Bounded IFO parsing. Physical cells remain in authored playback order.
//! Reference layout: libdvdread's ifo_types.h (PGC, PTT and cell_playback_t).
use media_rip_core::{
    Chapter,
    model::dvd::{DvdChapterRange, DvdTitleInfo},
};

use super::{
    DvdTitleReference, VTS_PGCIT_SECTOR_OFFSET, VTS_PTT_SRPT_SECTOR_OFFSET, read_be_u16,
    read_be_u32, table_at_sector_pointer,
};

#[derive(Debug)]
pub(super) struct Layout {
    pub info: DvdTitleInfo,
    pub chapters: Vec<Chapter>,
    pub size: u64,
    pub duplicate_key: String,
    pub map: String,
}

#[derive(Debug, Clone)]
struct Cell {
    chapter: u32,
    pgc: u16,
    number: usize,
    first: u32,
    last: u32,
    duration_ms: u64,
}

pub(super) fn analyse(
    bytes: &[u8],
    reference: DvdTitleReference,
    vob_sectors: Option<u64>,
) -> Result<Layout, String> {
    let programs =
        programs(bytes, reference.title_number).ok_or("Invalid chapter/program table")?;
    let pgcit = table_at_sector_pointer(bytes, VTS_PGCIT_SECTOR_OFFSET)
        .ok_or("Missing program-chain table")?;
    let pgc_count = read_be_u16(pgcit.get(..2).ok_or("Truncated program-chain table")?)
        .ok_or("Invalid program-chain count")?;
    let mut cells = Vec::new();
    for (chapter, &(pgcn, pgn)) in programs.iter().enumerate() {
        cells.extend(
            program_cells(pgcit, pgc_count, pgcn, pgn, chapter as u32 + 1)
                .ok_or_else(|| format!("Invalid cell table in chapter {}", chapter + 1))?,
        );
    }
    let first = cells
        .iter()
        .position(|cell| cell.duration_ms >= 1_000)
        .ok_or("Title contains only padding cells")?;
    let last = cells
        .iter()
        .rposition(|cell| cell.duration_ms >= 1_000)
        .ok_or("Title contains only padding cells")?;
    let leading = first as u32;
    let trailing = (cells.len() - last - 1) as u32;
    let cells = &cells[first..=last];
    if cells.iter().any(|cell| {
        cell.last < cell.first || vob_sectors.is_some_and(|sectors| u64::from(cell.last) >= sectors)
    }) {
        return Err("Cell addresses extend outside the title-set VOB data".to_owned());
    }
    let mut warnings = Vec::new();
    if vob_sectors.is_none() {
        warnings.push(
            "The title-set VOB extent is unavailable; cell addresses could not be checked"
                .to_owned(),
        );
    }
    let mut physical = cells
        .iter()
        .map(|cell| (cell.first, cell.last))
        .collect::<Vec<_>>();
    physical.sort_unstable();
    if physical.windows(2).any(|pair| pair[1].0 <= pair[0].1) {
        warnings.push(
            "Playback repeats or overlaps physical sectors; review this title before ripping"
                .to_owned(),
        );
    }
    // Backward jumps can be deliberate authoring. Flag them, never silently
    // sort the playback order or claim the title is a valid feature.
    if cells.windows(2).any(|pair| pair[1].first < pair[0].first) {
        warnings.push(
            "Cells jump backwards on disc; this may be an alternate or misleading playback path"
                .to_owned(),
        );
    }
    let mut size = 0_u64;
    let mut end = None;
    for (first, last) in physical {
        let first = end.map_or(first, |end: u32| first.max(end.saturating_add(1)));
        if first <= last {
            size = size
                .checked_add((u64::from(last) - u64::from(first) + 1) * 2048)
                .ok_or("Title size overflow")?;
        }
        end = Some(end.map_or(last, |end| end.max(last)));
    }
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut elapsed = 0_u64;
    for cell in cells {
        if chapters
            .last()
            .is_none_or(|chapter| chapter.index != cell.chapter)
        {
            chapters.push(Chapter {
                index: cell.chapter,
                start_ms: elapsed,
                end_ms: elapsed,
                name: None,
            });
        }
        elapsed = elapsed
            .checked_add(cell.duration_ms)
            .ok_or("Title duration overflow")?;
        chapters.last_mut().expect("chapter just inserted").end_ms = elapsed;
    }
    let range = DvdChapterRange {
        first: cells[0].chapter,
        last: cells.last().expect("nonempty cells").chapter,
    };
    // Chapter indices shown to the user start at zero after trimming; the
    // original DVD PTT coordinates are retained separately for extraction.
    for (index, chapter) in chapters.iter_mut().enumerate() {
        chapter.index = index as u32;
    }
    let sequence = cells
        .iter()
        .map(|cell| format!("{}-{}@{}", cell.first, cell.last, cell.duration_ms))
        .collect::<Vec<_>>()
        .join(",");
    Ok(Layout {
        info: DvdTitleInfo {
            title_set: reference.title_set,
            title_in_set: reference.title_number,
            chapter_range: Some(range),
            cell_count: cells.len() as u32,
            leading_padding_cells: leading,
            trailing_padding_cells: trailing,
            warnings,
        },
        chapters,
        size,
        duplicate_key: format!("dvd-vts{}:{sequence}", reference.title_set),
        map: cells
            .iter()
            .map(|cell| {
                format!(
                    "PGC {} cell {}: sectors {}–{}",
                    cell.pgc, cell.number, cell.first, cell.last
                )
            })
            .collect::<Vec<_>>()
            .join("; "),
    })
}

fn programs(bytes: &[u8], title: u8) -> Option<Vec<(u16, u16)>> {
    let table = table_at_sector_pointer(bytes, VTS_PTT_SRPT_SECTOR_OFFSET)?;
    let count = usize::from(read_be_u16(table.get(..2)?)?);
    let index = usize::from(title).checked_sub(1)?;
    if count == 0 || count > 99 || index >= count {
        return None;
    }
    let header_end = 8 + count * 4;
    let start = read_be_u32(table.get(8 + index * 4..12 + index * 4)?)? as usize;
    let end = if index + 1 < count {
        read_be_u32(table.get(12 + index * 4..16 + index * 4)?)? as usize
    } else {
        (read_be_u32(table.get(4..8)?)? as usize).checked_add(1)?
    };
    if start < header_end
        || start >= end
        || !(end - start).is_multiple_of(4)
        || (end - start) / 4 > 99
    {
        return None;
    }
    table
        .get(start..end)?
        .chunks_exact(4)
        .map(|entry| {
            let pgc = read_be_u16(&entry[..2])?;
            let pg = read_be_u16(&entry[2..])?;
            (pgc > 0 && pg > 0).then_some((pgc, pg))
        })
        .collect()
}

fn program_cells(pgcit: &[u8], count: u16, pgcn: u16, pgn: u16, chapter: u32) -> Option<Vec<Cell>> {
    if pgcn == 0 || pgcn > count || count > 999 {
        return None;
    }
    let pointer = 8 + (usize::from(pgcn) - 1) * 8;
    let offset = read_be_u32(pgcit.get(pointer + 4..pointer + 8)?)? as usize;
    if offset < 8 + usize::from(count) * 8 {
        return None;
    }
    let pgc = pgcit.get(offset..)?;
    let program_count = usize::from(*pgc.get(2)?);
    let cell_count = usize::from(*pgc.get(3)?);
    let program = usize::from(pgn);
    if program == 0 || program > program_count || cell_count == 0 {
        return None;
    }
    let map = read_be_u16(pgc.get(230..232)?)? as usize;
    let playback = read_be_u16(pgc.get(232..234)?)? as usize;
    if map < 236 || playback < map + program_count {
        return None;
    }
    let first = usize::from(*pgc.get(map + program - 1)?);
    let next = if program < program_count {
        usize::from(*pgc.get(map + program)?)
    } else {
        cell_count + 1
    };
    if first == 0 || first >= next || next > cell_count + 1 {
        return None;
    }
    let mut cells = Vec::new();
    for number in first..next {
        let cell = pgc.get(playback + (number - 1) * 24..playback + number * 24)?;
        let block_type = (cell[0] >> 4) & 3;
        let block_mode = cell[0] >> 6;
        if block_type > 1 {
            return None;
        }
        if block_type == 1 && block_mode != 1 {
            continue;
        }
        let first = read_be_u32(&cell[8..12])?;
        let last = read_be_u32(&cell[20..24])?;
        cells.push(Cell {
            chapter,
            pgc: pgcn,
            number,
            first,
            last,
            duration_ms: dvd_time(&cell[4..8])?,
        });
    }
    Some(cells)
}

fn dvd_time(bytes: &[u8]) -> Option<u64> {
    let bcd = |value: u8| {
        ((value & 15) < 10 && (value >> 4) < 10)
            .then_some(u64::from(value >> 4) * 10 + u64::from(value & 15))
    };
    let hours = bcd(bytes[0])?;
    let minutes = bcd(bytes[1])?;
    let seconds = bcd(bytes[2])?;
    if minutes >= 60 || seconds >= 60 {
        return None;
    }
    let frames = bcd(bytes[3] & 0x3f)?;
    let frame_ms = match bytes[3] >> 6 {
        1 if frames < 25 => frames * 40,
        3 if frames < 30 => frames * 1001 / 30,
        0 if frames == 0 => 0,
        _ => return None,
    };
    Some((hours * 3600 + minutes * 60 + seconds) * 1000 + frame_ms)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(in crate::source::dvd) fn fixture(cells: &[(u64, u32, u32)]) -> Vec<u8> {
        let mut bytes = vec![0; 8192];
        bytes[..12].copy_from_slice(b"DVDVIDEO-VTS");
        bytes[VTS_PTT_SRPT_SECTOR_OFFSET..VTS_PTT_SRPT_SECTOR_OFFSET + 4]
            .copy_from_slice(&1_u32.to_be_bytes());
        bytes[VTS_PGCIT_SECTOR_OFFSET..VTS_PGCIT_SECTOR_OFFSET + 4]
            .copy_from_slice(&2_u32.to_be_bytes());
        let ptt = 2048;
        bytes[ptt..ptt + 2].copy_from_slice(&1_u16.to_be_bytes());
        bytes[ptt + 4..ptt + 8].copy_from_slice(&(11 + cells.len() as u32 * 4).to_be_bytes());
        bytes[ptt + 8..ptt + 12].copy_from_slice(&12_u32.to_be_bytes());
        for index in 0..cells.len() {
            let pos = ptt + 12 + index * 4;
            bytes[pos..pos + 2].copy_from_slice(&1_u16.to_be_bytes());
            bytes[pos + 2..pos + 4].copy_from_slice(&(index as u16 + 1).to_be_bytes());
        }
        let table = 4096;
        bytes[table..table + 2].copy_from_slice(&1_u16.to_be_bytes());
        bytes[table + 12..table + 16].copy_from_slice(&16_u32.to_be_bytes());
        let pgc = table + 16;
        bytes[pgc + 2] = cells.len() as u8;
        bytes[pgc + 3] = cells.len() as u8;
        bytes[pgc + 230..pgc + 232].copy_from_slice(&236_u16.to_be_bytes());
        bytes[pgc + 232..pgc + 234].copy_from_slice(&(236 + cells.len() as u16).to_be_bytes());
        let bcd = |value: u64| (((value / 10) << 4) | (value % 10)) as u8;
        for (index, &(ms, first, last)) in cells.iter().enumerate() {
            bytes[pgc + 236 + index] = index as u8 + 1;
            let cell = pgc + 236 + cells.len() + index * 24;
            bytes[cell + 4..cell + 8].copy_from_slice(&[
                bcd(ms / 3_600_000),
                bcd(ms / 60_000 % 60),
                bcd(ms / 1000 % 60),
                0x40,
            ]);
            bytes[cell + 8..cell + 12].copy_from_slice(&first.to_be_bytes());
            bytes[cell + 20..cell + 24].copy_from_slice(&last.to_be_bytes());
        }
        bytes
    }

    fn reference() -> DvdTitleReference {
        DvdTitleReference {
            title_set: 1,
            title_number: 1,
        }
    }

    #[test]
    fn trims_the_supplied_padding_pattern_without_changing_authored_coordinates() {
        // A synthetic layout reproducing the user's MakeMKV report: eight
        // leading cells, sixteen content cells totalling 1:30:24, one trailing.
        let mut cells = vec![(0, u32::MAX, 0); 8];
        cells.extend((0..16).map(|index| (339_000, index * 100, index * 100 + 99)));
        cells.push((0, u32::MAX, 0));
        let layout = analyse(&fixture(&cells), reference(), Some(1600)).unwrap();
        assert_eq!(
            layout.info.chapter_range,
            Some(DvdChapterRange { first: 9, last: 24 })
        );
        assert_eq!(layout.info.cell_count, 16);
        assert_eq!(layout.info.leading_padding_cells, 8);
        assert_eq!(layout.info.trailing_padding_cells, 1);
        assert_eq!(layout.chapters.last().unwrap().end_ms, 5_424_000);
        assert_eq!(layout.size, 1600 * 2048);
        assert!(layout.info.warnings.is_empty());
    }

    #[test]
    fn preserves_program_order_and_flags_repeated_or_backward_cells() {
        let normal = fixture(&[(60_000, 0, 99), (60_000, 100, 199)]);
        let mut backwards = normal.clone();
        backwards[2062..2064].copy_from_slice(&2_u16.to_be_bytes());
        backwards[2066..2068].copy_from_slice(&1_u16.to_be_bytes());
        let first = analyse(&normal, reference(), Some(200)).unwrap();
        let second = analyse(&backwards, reference(), Some(200)).unwrap();
        assert_ne!(first.duplicate_key, second.duplicate_key);
        assert!(!second.info.warnings.is_empty());
        let repeated = analyse(
            &fixture(&[(60_000, 0, 99), (60_000, 0, 99)]),
            reference(),
            Some(200),
        )
        .unwrap();
        assert_eq!(repeated.size, 100 * 2048);
        assert!(!repeated.info.warnings.is_empty());
    }

    #[test]
    fn rejects_out_of_bounds_cells_and_padding_only_titles() {
        assert!(analyse(&fixture(&[(60_000, 0, 500)]), reference(), Some(100)).is_err());
        assert!(analyse(&fixture(&[(0, 0, 100)]), reference(), Some(101)).is_err());
        assert!(analyse(&fixture(&[(60_000, 100, 0)]), reference(), Some(101)).is_err());
    }

    #[test]
    fn validates_bcd_frame_rates() {
        assert_eq!(dvd_time(&[0x01, 0x30, 0x24, 0x40]), Some(5_424_000));
        assert_eq!(dvd_time(&[0, 0, 0, 0x50]), Some(400));
        assert_eq!(dvd_time(&[0, 0x60, 0, 0x40]), None);
        assert_eq!(dvd_time(&[0, 0, 0, 0x65]), None);
    }
}
