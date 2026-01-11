use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildCommandSource {
    BuildSh,
    Npm,
    Composer,
}

pub struct BuildCommandCandidate {
    pub source: BuildCommandSource,
    pub command: String,
}

fn file_exists(path: &Path) -> bool {
    std::fs::metadata(path).is_ok()
}

fn artifact_is_zip(artifact_path: &str) -> bool {
    artifact_path.to_ascii_lowercase().ends_with(".zip")
}

pub fn detect_build_command(
    local_path: &str,
    build_artifact: &str,
) -> Option<BuildCommandCandidate> {
    let root = PathBuf::from(local_path);

    if artifact_is_zip(build_artifact) {
        let root_build = root.join("build.sh");
        if file_exists(&root_build) {
            return Some(BuildCommandCandidate {
                source: BuildCommandSource::BuildSh,
                command: "sh build.sh".to_string(),
            });
        }
    }

    None
}

pub fn detect_zip_single_root_dir(zip_path: &Path) -> crate::Result<Option<String>> {
    let file = std::fs::File::open(zip_path)
        .map_err(|err| crate::Error::internal_io(err.to_string(), Some("open zip".to_string())))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|err| crate::Error::internal_unexpected(format!("Zip parse error: {}", err)))?;

    let mut roots: BTreeSet<String> = BTreeSet::new();

    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|err| {
            crate::Error::internal_unexpected(format!("Zip entry error: {}", err))
        })?;
        let name = entry.name();

        let mut parts = name.split('/').filter(|p| !p.is_empty());
        let Some(first) = parts.next() else {
            continue;
        };

        if first == "__MACOSX" || first == ".DS_Store" {
            continue;
        }

        roots.insert(first.to_string());

        if roots.len() > 1 {
            return Ok(None);
        }
    }

    Ok(roots.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn detects_build_sh_for_zip_artifacts() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::write(temp_dir.path().join("build.sh"), "#!/bin/sh\necho ok\n").unwrap();

        let candidate =
            detect_build_command(temp_dir.path().to_str().unwrap(), "dist/plugin.zip").unwrap();
        assert_eq!(candidate.source, BuildCommandSource::BuildSh);
        assert_eq!(candidate.command, "sh build.sh");
    }

    #[test]
    fn does_not_detect_build_sh_for_non_zip_artifacts() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::write(temp_dir.path().join("build.sh"), "#!/bin/sh\necho ok\n").unwrap();

        assert!(detect_build_command(temp_dir.path().to_str().unwrap(), "dist/app.js").is_none());
    }

    #[test]
    fn detects_single_root_dir_in_zip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let zip_path = temp_dir.path().join("plugin.zip");

        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::FileOptions::default();

            zip.add_directory("sell-my-images/", options).unwrap();
            zip.start_file("sell-my-images/sell-my-images.php", options)
                .unwrap();
            zip.write_all(b"<?php\n/*\nPlugin Name: Sell My Images\n*/\n")
                .unwrap();
            zip.finish().unwrap();
        }

        let root = detect_zip_single_root_dir(&zip_path).unwrap();
        assert_eq!(root.as_deref(), Some("sell-my-images"));
    }

    #[test]
    fn returns_none_for_multiple_root_dirs() {
        let temp_dir = tempfile::tempdir().unwrap();
        let zip_path = temp_dir.path().join("mixed.zip");

        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::FileOptions::default();

            zip.start_file("one/a.txt", options).unwrap();
            zip.write_all(b"a").unwrap();
            zip.start_file("two/b.txt", options).unwrap();
            zip.write_all(b"b").unwrap();
            zip.finish().unwrap();
        }

        let root = detect_zip_single_root_dir(&zip_path).unwrap();
        assert!(root.is_none());
    }
}
