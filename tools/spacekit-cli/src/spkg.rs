use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Seek, Write};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use spacekit_primitives::v1::app::AppPackage;
use zip::write::FileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

pub const MEDIA_TYPE: &str = "application/vnd.spacekit.spkg+zip";
pub const MIMETYPE: &[u8] = MEDIA_TYPE.as_bytes();
const MAX_ENTRIES: usize = 10_000;
const MAX_UNCOMPRESSED_SIZE: u64 = 512 * 1024 * 1024;

pub type PackageFiles = BTreeMap<String, Vec<u8>>;

fn validate_contents(package: &AppPackage, files: &PackageFiles) -> Result<()> {
    let mut referenced = HashSet::with_capacity(package.content_refs.len());
    let mut total_size = 0_u64;
    let mut aggregate = Sha256::new();

    for content_ref in &package.content_refs {
        validate_payload_path(&content_ref.path)?;
        if !referenced.insert(content_ref.path.as_str()) {
            bail!("duplicate AppPackage content ref: {}", content_ref.path);
        }
        let data = files
            .get(&content_ref.path)
            .ok_or_else(|| anyhow::anyhow!("missing SPKG payload: {}", content_ref.path))?;
        if data.len() as u64 != content_ref.size {
            bail!(
                "payload size mismatch for {}: manifest says {}, payload is {}",
                content_ref.path,
                content_ref.size,
                data.len()
            );
        }
        let actual_hash: [u8; 32] = Sha256::digest(data).into();
        if actual_hash != content_ref.hash {
            bail!("payload hash mismatch for {}", content_ref.path);
        }
        total_size = total_size
            .checked_add(content_ref.size)
            .ok_or_else(|| anyhow::anyhow!("manifest payload size overflow"))?;
        aggregate.update(content_ref.hash);
    }

    if files.len() != referenced.len() {
        let extra = files
            .keys()
            .find(|path| !referenced.contains(path.as_str()))
            .map(String::as_str)
            .unwrap_or("<unknown>");
        bail!("unreferenced SPKG payload: {extra}");
    }
    if total_size != package.manifest.total_size {
        bail!(
            "manifest total_size mismatch: manifest says {}, content refs total {}",
            package.manifest.total_size,
            total_size
        );
    }
    let aggregate_hash: [u8; 32] = aggregate.finalize().into();
    if aggregate_hash != package.manifest.checksum {
        bail!("manifest checksum mismatch");
    }
    Ok(())
}

/// Write an SPKG v1 archive containing an AppPackage and its payload files.
pub fn write<W: Write + Seek>(
    destination: W,
    package: &AppPackage,
    files: &PackageFiles,
) -> Result<W> {
    validate_contents(package, files)?;
    if files.len().saturating_add(2) > MAX_ENTRIES {
        bail!("SPKG contains more than {MAX_ENTRIES} entries");
    }

    let manifest = serde_json::to_vec_pretty(package).context("serialize SPKG manifest")?;
    let mut total_size = MIMETYPE.len() as u64 + manifest.len() as u64;
    for (path, data) in files {
        validate_payload_path(path)?;
        total_size = total_size
            .checked_add(data.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("SPKG uncompressed size overflow"))?;
        if total_size > MAX_UNCOMPRESSED_SIZE {
            bail!("SPKG exceeds the 512 MiB uncompressed size limit");
        }
    }

    let metadata = FileOptions::default()
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    let stored = metadata.compression_method(CompressionMethod::Stored);
    let deflated = metadata.compression_method(CompressionMethod::Deflated);

    let mut archive = ZipWriter::new(destination);
    archive.start_file("mimetype", stored)?;
    archive.write_all(MIMETYPE)?;
    archive.start_file("manifest.json", deflated)?;
    archive.write_all(&manifest)?;

    // BTreeMap iteration gives a stable lexical payload order.
    for (path, data) in files {
        archive.start_file(format!("payload/{path}"), deflated)?;
        archive.write_all(data)?;
    }

    archive.finish().context("finish SPKG archive")
}

/// Read and validate an SPKG v1 archive.
pub fn read<R: Read + Seek>(source: R) -> Result<(AppPackage, PackageFiles)> {
    let mut archive = ZipArchive::new(source).context("open SPKG ZIP archive")?;
    if archive.len() > MAX_ENTRIES {
        bail!("SPKG contains more than {MAX_ENTRIES} entries");
    }
    if archive.len() < 2 {
        bail!("SPKG is missing required entries");
    }

    let mut seen = HashSet::with_capacity(archive.len());
    let mut manifest = None;
    let mut files = PackageFiles::new();
    let mut total_size = 0_u64;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = std::str::from_utf8(entry.name_raw())
            .context("SPKG entry names must be UTF-8")?
            .to_owned();
        let declared_size = entry.size();

        if !seen.insert(name.clone()) {
            bail!("duplicate SPKG entry: {name}");
        }
        let is_symlink = entry
            .unix_mode()
            .map(|mode| mode & 0o170000 == 0o120000)
            .unwrap_or(false);
        if entry.is_dir() || is_symlink {
            bail!("SPKG non-file entries are not allowed: {name}");
        }
        if !matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            bail!("unsupported ZIP compression for SPKG entry: {name}");
        }
        if index == 0 && (name != "mimetype" || entry.compression() != CompressionMethod::Stored) {
            bail!("first SPKG entry must be an uncompressed mimetype");
        }

        if declared_size > MAX_UNCOMPRESSED_SIZE - total_size {
            bail!("SPKG exceeds the 512 MiB uncompressed size limit");
        }

        let remaining = MAX_UNCOMPRESSED_SIZE - total_size;
        let mut data = Vec::with_capacity(declared_size.min(remaining) as usize);
        entry
            .take(remaining + 1)
            .read_to_end(&mut data)
            .with_context(|| format!("read SPKG entry {name}"))?;
        if data.len() as u64 > remaining {
            bail!("SPKG exceeds the 512 MiB uncompressed size limit");
        }
        if data.len() as u64 != declared_size {
            bail!("SPKG entry size differs from ZIP metadata: {name}");
        }
        total_size += data.len() as u64;

        match name.as_str() {
            "mimetype" => {
                if index != 0 || data != MIMETYPE {
                    bail!("invalid SPKG mimetype entry");
                }
            }
            "manifest.json" => {
                if manifest.replace(data).is_some() {
                    bail!("duplicate SPKG manifest");
                }
            }
            _ if name.starts_with("signatures/") => {
                validate_payload_path(
                    name.strip_prefix("signatures/")
                        .expect("prefix was checked"),
                )?;
            }
            _ => {
                let path = name
                    .strip_prefix("payload/")
                    .ok_or_else(|| anyhow::anyhow!("unexpected SPKG entry: {name}"))?;
                validate_payload_path(path)?;
                if files.insert(path.to_owned(), data).is_some() {
                    bail!("duplicate SPKG payload path: {path}");
                }
            }
        }
    }

    let manifest = manifest.ok_or_else(|| anyhow::anyhow!("SPKG is missing manifest.json"))?;
    let package =
        serde_json::from_slice(&manifest).context("parse AppPackage from SPKG manifest")?;
    validate_contents(&package, &files)?;
    Ok((package, files))
}

pub fn validate_payload_path(path: &str) -> Result<()> {
    if path.is_empty() {
        bail!("SPKG payload path cannot be empty");
    }
    if path.starts_with('/') || path.starts_with('\\') {
        bail!("SPKG payload path cannot be absolute: {path}");
    }
    if path.contains('\\') || path.contains('\0') {
        bail!("SPKG payload path contains an invalid character: {path}");
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        bail!("SPKG payload path contains an unsafe segment: {path}");
    }
    if path.as_bytes().get(1) == Some(&b':') && path.as_bytes()[0].is_ascii_alphabetic() {
        bail!("SPKG payload path cannot be absolute: {path}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use spacekit_primitives::v1::app::{
        AppCategory, AppManifest, AppPricing, CompressionAlgorithm, ContentRef, ContentType,
        EntryPoint, Platform, SemVer,
    };
    use spacekit_primitives::v1::crypto::quantum::SPHINCSSignature;
    use spacekit_primitives::v1::fact::{AccessPolicy, LicenseType};
    use spacekit_primitives::v1::identity::QuantumDID;
    use std::io::Cursor;

    fn package(files: &PackageFiles) -> AppPackage {
        let creator_did = QuantumDID::new("did:swtchx:spkg-test".to_owned());
        let mut aggregate = Sha256::new();
        let mut total_size = 0;
        let content_refs = files
            .iter()
            .map(|(path, data)| {
                let hash: [u8; 32] = Sha256::digest(data).into();
                aggregate.update(hash);
                total_size += data.len() as u64;
                ContentRef {
                    path: path.clone(),
                    content_type: ContentType::from_extension(
                        path.rsplit_once('.').map(|(_, ext)| ext).unwrap_or(""),
                    ),
                    size: data.len() as u64,
                    hash,
                    compression: CompressionAlgorithm::None,
                    encrypted: false,
                    fact_id: [0; 32],
                }
            })
            .collect();
        AppPackage {
            app_id: AppPackage::compute_app_id(&creator_did, "SPKG test"),
            version: SemVer::new(1, 0, 0),
            created_at: 1,
            creator_did,
            signature: SPHINCSSignature::new(Vec::new(), "SPHINCS-256f".to_owned(), Vec::new()),
            manifest: AppManifest {
                name: "SPKG test".to_owned(),
                description: String::new(),
                tagline: None,
                entry_points: vec![EntryPoint::Html {
                    path: "index.html".to_owned(),
                    is_main: true,
                }],
                permissions: Vec::new(),
                content_types: vec![ContentType::Html],
                total_size,
                checksum: aggregate.finalize().into(),
                icon: None,
                screenshots: Vec::new(),
                keywords: Vec::new(),
                min_runtime_version: None,
                platforms: vec![Platform::Web],
            },
            content_refs,
            license_type: LicenseType::MIT,
            access_policy: AccessPolicy::Public,
            dependencies: Vec::new(),
            category: AppCategory::Utilities,
            pricing: AppPricing::Free,
        }
    }

    fn unchecked_archive(
        package: &AppPackage,
        files: &PackageFiles,
        include_signature: bool,
    ) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let stored = FileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = FileOptions::default().compression_method(CompressionMethod::Deflated);
        writer.start_file("mimetype", stored).unwrap();
        writer.write_all(MIMETYPE).unwrap();
        writer.start_file("manifest.json", deflated).unwrap();
        writer
            .write_all(&serde_json::to_vec(package).unwrap())
            .unwrap();
        for (path, data) in files {
            writer
                .start_file(format!("payload/{path}"), deflated)
                .unwrap();
            writer.write_all(data).unwrap();
        }
        if include_signature {
            writer
                .start_file("signatures/manifest.sig", stored)
                .unwrap();
            writer.write_all(b"signature").unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn round_trip_archive() {
        let files = PackageFiles::from([
            ("scripts/app.js".to_owned(), b"export {}".to_vec()),
            ("index.html".to_owned(), b"<h1>Hi</h1>\n".to_vec()),
        ]);
        let expected_package = package(&files);

        let archive = write(Cursor::new(Vec::new()), &expected_package, &files)
            .unwrap()
            .into_inner();
        let mut zip = ZipArchive::new(Cursor::new(&archive)).unwrap();
        assert_eq!(zip.by_index(0).unwrap().name(), "mimetype");
        assert_eq!(
            zip.by_index(0).unwrap().compression(),
            CompressionMethod::Stored
        );
        drop(zip);

        let (actual_package, actual_files) = read(Cursor::new(archive)).unwrap();
        assert_eq!(actual_package, expected_package);
        assert_eq!(actual_files, files);

        let signed = unchecked_archive(&expected_package, &files, true);
        assert!(read(Cursor::new(signed)).is_ok());
    }

    #[test]
    fn rejects_unsafe_payload_paths() {
        for path in [
            "",
            "/etc/passwd",
            r"C:\Windows\file",
            "foo\\bar",
            "foo//bar",
            "./foo",
            "foo/../bar",
            "foo/.",
        ] {
            assert!(validate_payload_path(path).is_err(), "{path} was accepted");
        }
        assert!(validate_payload_path("assets/icons/app.png").is_ok());

        let files = PackageFiles::from([("../escape".to_owned(), Vec::new())]);
        let empty = PackageFiles::new();
        assert!(write(Cursor::new(Vec::new()), &package(&empty), &files).is_err());
    }

    #[test]
    fn rejects_mismatched_payload_hash() {
        let files = PackageFiles::from([("index.html".to_owned(), b"actual".to_vec())]);
        let mut bad_package = package(&files);
        bad_package.content_refs[0].hash = [7; 32];

        assert!(write(Cursor::new(Vec::new()), &bad_package, &files).is_err());
        let archive = unchecked_archive(&bad_package, &files, false);
        assert!(read(Cursor::new(archive)).is_err());
    }

    #[test]
    fn rejects_missing_payload() {
        let files = PackageFiles::from([("index.html".to_owned(), b"content".to_vec())]);
        let package = package(&files);
        let archive = unchecked_archive(&package, &PackageFiles::new(), false);
        assert!(read(Cursor::new(archive)).is_err());
    }

    #[test]
    fn rejects_extra_payload() {
        let files = PackageFiles::from([("index.html".to_owned(), b"content".to_vec())]);
        let package = package(&files);
        let mut archive_files = files;
        archive_files.insert("extra.js".to_owned(), b"extra".to_vec());
        let archive = unchecked_archive(&package, &archive_files, false);
        assert!(read(Cursor::new(archive)).is_err());
    }
}
