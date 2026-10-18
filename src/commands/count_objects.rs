use anyhow::Result;
use crate::helpers::count_objects::{collect_object_counts, format_human_readable_size};

/// `rgit count-objects [-v|--verbose] [-H|--human-readable]`
///
/// Counts unpacked (loose) objects and reports their disk consumption. With `-v`, additionally
/// reports packed objects, number of packfiles, pack sizes, prune-packable loose objects, and
/// garbage files found in the object database. With `-H`, formats byte sizes using human-readable
/// units (bytes, KiB, MiB, GiB).
pub fn count_objects(verbose: bool, human_readable: bool) -> Result<()> {
    let report = collect_object_counts(verbose)?;

    if !verbose {
        if human_readable {
            println!("{} objects, {}", report.count, format_human_readable_size(report.size_bytes));
        } else {
            println!("{} objects, {} kilobytes", report.count, report.size_bytes / 1024);
        }
    } else {
        println!("count: {}", report.count);
        if human_readable {
            println!("size: {}", format_human_readable_size(report.size_bytes));
        } else {
            println!("size: {}", report.size_bytes / 1024);
        }
        println!("in-pack: {}", report.in_pack);
        println!("packs: {}", report.packs);
        if human_readable {
            println!("size-pack: {}", format_human_readable_size(report.size_pack_bytes));
        } else {
            println!("size-pack: {}", report.size_pack_bytes / 1024);
        }
        println!("prune-packable: {}", report.prune_packable);
        println!("garbage: {}", report.garbage);
        if human_readable {
            println!("size-garbage: {}", format_human_readable_size(report.size_garbage_bytes));
        } else {
            println!("size-garbage: {}", report.size_garbage_bytes / 1024);
        }
    }

    Ok(())
}
