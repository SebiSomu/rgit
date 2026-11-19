/// Statistics collected by the `count-objects` command about loose objects, packs, and garbage.
// Used by: src/storage/count_objects/engine.rs
// Used by command/helper: `collect_object_counts`, `format_human_readable_size`, and the `count_objects` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectCountReport {
    pub count: usize,
    pub size_bytes: u64,
    pub in_pack: usize,
    pub packs: usize,
    pub size_pack_bytes: u64,
    pub prune_packable: usize,
    pub garbage: usize,
    pub size_garbage_bytes: u64,
}
