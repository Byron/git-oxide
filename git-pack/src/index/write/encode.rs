use crate::index::{util::Count, V2_SIGNATURE};
use byteorder::{BigEndian, WriteBytesExt};
use git_features::{
    hash,
    progress::{self, Progress},
};
use std::{cmp::Ordering, collections::VecDeque, io};

pub(crate) fn write_to(
    out: impl io::Write,
    entries_sorted_by_oid: VecDeque<crate::tree::Item<crate::index::write::TreeEntry>>,
    pack_hash: &git_hash::ObjectId,
    kind: crate::index::Version,
    mut progress: impl Progress,
) -> io::Result<git_hash::ObjectId> {
    use io::Write;
    assert!(
        !entries_sorted_by_oid.is_empty(),
        "Empty packs do not exists, or so I think"
    );
    assert_eq!(kind, crate::index::Version::V2, "Can only write V2 packs right now");
    assert!(
        entries_sorted_by_oid.len() <= u32::MAX as usize,
        "a pack cannot have more than u32::MAX objects"
    );

    // Write header
    let mut out = Count::new(std::io::BufWriter::with_capacity(
        8 * 4096,
        hash::Write::new(out, kind.hash()),
    ));
    out.write_all(V2_SIGNATURE)?;
    out.write_u32::<BigEndian>(kind as u32)?;

    const LARGE_OFFSET_THRESHOLD: u64 = 0x7fff_ffff;
    const HIGH_BIT: u32 = 0x8000_0000;

    let needs_64bit_offsets =
        entries_sorted_by_oid.back().expect("at least one pack entry").offset > LARGE_OFFSET_THRESHOLD;
    let mut fan_out = [0u32; 256];
    progress.init(Some(4), progress::steps());
    let start = std::time::Instant::now();
    let _info = progress.add_child("generating fan-out table");

    {
        let mut iter = entries_sorted_by_oid.iter().enumerate();
        let mut idx_and_entry = iter.next();
        let mut upper_bound = 0;
        let entries_len = entries_sorted_by_oid.len() as u32;

        for (offset_be, byte) in fan_out.iter_mut().zip(0u8..=255) {
            *offset_be = match idx_and_entry.as_ref() {
                Some((_idx, entry)) => match entry.data.id.as_slice()[0].cmp(&byte) {
                    Ordering::Less => unreachable!("ids should be ordered, and we make sure to keep ahead with them"),
                    Ordering::Greater => upper_bound,
                    Ordering::Equal => {
                        idx_and_entry = iter.find(|(_, entry)| entry.data.id.as_slice()[0] != byte);
                        upper_bound = match idx_and_entry.as_ref() {
                            Some((idx, _)) => *idx as u32,
                            None => entries_len,
                        };
                        upper_bound
                    }
                },
                None => entries_len,
            };
        }
    }

    for value in fan_out {
        out.write_u32::<BigEndian>(value)?;
    }

    progress.inc();
    let _info = progress.add_child("writing ids");
    for entry in &entries_sorted_by_oid {
        out.write_all(entry.data.id.as_slice())?;
    }

    progress.inc();
    let _info = progress.add_child("writing crc32");
    for entry in &entries_sorted_by_oid {
        out.write_u32::<BigEndian>(entry.data.crc32)?;
    }

    progress.inc();
    let _info = progress.add_child("writing offsets");
    {
        let mut offsets64 = Vec::<u64>::new();
        for entry in &entries_sorted_by_oid {
            out.write_u32::<BigEndian>(if needs_64bit_offsets && entry.offset > LARGE_OFFSET_THRESHOLD {
                assert!(
                    offsets64.len() < LARGE_OFFSET_THRESHOLD as usize,
                    "Encoding breakdown - way too many 64bit offsets"
                );
                offsets64.push(entry.offset);
                ((offsets64.len() - 1) as u32) | HIGH_BIT
            } else {
                entry.offset as u32
            })?;
        }
        if needs_64bit_offsets {
            for value in offsets64 {
                out.write_u64::<BigEndian>(value)?;
            }
        }
    }

    out.write_all(pack_hash.as_slice())?;

    let bytes_written_without_trailer = out.bytes;
    let mut out = out.inner.into_inner()?;
    let index_hash: git_hash::ObjectId = out.hash.digest().into();
    out.inner.write_all(index_hash.as_slice())?;
    out.inner.flush()?;

    progress.inc();
    progress.show_throughput_with(
        start,
        (bytes_written_without_trailer + 20) as usize,
        progress::bytes().expect("unit always set"),
    );

    Ok(index_hash)
}
