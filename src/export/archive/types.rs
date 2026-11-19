use std::path::Path;

/// Represents the supported serialization archive format (tar, tar.gz, or zip).
// Used by: src/export/archive/types.rs, src/export/archive.rs, src/export/archive/engine.rs
// Used by command/helper: `create_archive`, `ArchiveFormat::parse`, and the `archive` command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Tar,
    TarGz,
    Zip,
}

impl ArchiveFormat {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "tar" => Ok(ArchiveFormat::Tar),
            "tar.gz" | "tgz" => Ok(ArchiveFormat::TarGz),
            "zip" => Ok(ArchiveFormat::Zip),
            other => anyhow::bail!("Unknown archive format '{}'", other),
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?.to_ascii_lowercase();
        if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            Some(ArchiveFormat::TarGz)
        } else if name.ends_with(".tar") {
            Some(ArchiveFormat::Tar)
        } else if name.ends_with(".zip") {
            Some(ArchiveFormat::Zip)
        } else {
            None
        }
    }

    // Unused before the refactor too (hidden while it lived in the `pub` objects.rs).
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            ArchiveFormat::Tar => "tar",
            ArchiveFormat::TarGz => "tar.gz",
            ArchiveFormat::Zip => "zip",
        }
    }
}
