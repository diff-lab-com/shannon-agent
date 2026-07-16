//! Installer for packaged Desktop Extensions (`.dxt` / `.mcpb`).
//!
//! Both formats are ZIP archives containing a plugin manifest plus its
//! asset files. The installer extracts the archive into the plugins
//! directory under the plugin's declared name, then returns the parsed
//! manifest so the caller can register it.
//!
//! Layout supported (any of):
//! - `manifest.json` at the archive root (Shannon / Claude-compatible)
//! - `.claude-plugin/plugin.json` (Claude Code ecosystem)
//! - `plugin.toml` (Shannon native, rare inside archives but accepted)

use std::fs as std_fs;
use std::io::{self, Cursor, Read};
use std::path::{Component, Path, PathBuf};

use super::{PluginError, PluginManifest, PluginResult};

/// Recognized Desktop Extension archive kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionKind {
    /// Anthropic Desktop Extension (`.dxt`).
    Dxt,
    /// MCP Bundle (`.mcpb`).
    Mcpb,
}

impl ExtensionKind {
    /// Infer the archive kind from a file extension. Returns `None` for
    /// unrecognized extensions so callers can decide whether to reject.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "dxt" => Some(Self::Dxt),
            "mcpb" => Some(Self::Mcpb),
            _ => None,
        }
    }

    /// File extension (without leading dot), e.g. `"dxt"`.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Dxt => "dxt",
            Self::Mcpb => "mcpb",
        }
    }
}

/// Manifest locations inside the archive, tried in priority order.
const MANIFEST_PATHS: &[&str] = &["plugin.toml", ".claude-plugin/plugin.json", "manifest.json"];

/// Parse manifest bytes using the format implied by the entry path.
fn parse_manifest_named(path: &str, bytes: &[u8]) -> PluginResult<PluginManifest> {
    if path.ends_with(".toml") {
        PluginManifest::from_toml_bytes(bytes).map_err(PluginError::InvalidManifest)
    } else {
        PluginManifest::from_json_bytes(bytes).map_err(PluginError::InvalidManifest)
    }
}

/// Locate and parse the first available manifest in the archive.
///
/// Tries Shannon TOML, then Claude's `.claude-plugin/plugin.json`, then a
/// root `manifest.json` (the canonical Desktop Extension location).
fn find_manifest<R: Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> PluginResult<PluginManifest> {
    for path in MANIFEST_PATHS {
        if let Ok(mut entry) = archive.by_name(path) {
            let mut buf = Vec::with_capacity(4 * 1024);
            entry.read_to_end(&mut buf).map_err(PluginError::Io)?;
            return parse_manifest_named(path, &buf);
        }
    }
    Err(PluginError::InvalidManifest(format!(
        "no manifest found in archive (looked for: {})",
        MANIFEST_PATHS.join(", ")
    )))
}

/// Sanitize an archive entry path against path traversal.
///
/// Rejects absolute paths and any segment containing `..` so an archive
/// cannot escape the destination directory. Returns the sanitized relative
/// path or an error.
fn sanitize_entry_path(rel: &str) -> PluginResult<PathBuf> {
    let p = Path::new(rel);
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(PluginError::InvalidManifest(format!(
                    "archive entry '{rel}' attempts path traversal"
                )));
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(PluginError::InvalidManifest(format!(
            "archive entry '{rel}' has empty path"
        )));
    }
    Ok(out)
}

/// Extract a ZIP archive into `dest`, refusing traversal entries.
fn extract_zip<R: Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    dest: &Path,
) -> PluginResult<()> {
    std_fs::create_dir_all(dest).map_err(PluginError::Io)?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| PluginError::InvalidManifest(format!("zip entry {i}: {e}")))?;
        let raw_name = entry.name().to_string();
        if raw_name.ends_with('/') {
            // Directory entry — ensure it exists after sanitization.
            if let Ok(safe) = sanitize_entry_path(raw_name.trim_end_matches('/')) {
                std_fs::create_dir_all(dest.join(safe)).map_err(PluginError::Io)?;
            }
            continue;
        }
        let safe = sanitize_entry_path(&raw_name)?;
        let target = dest.join(&safe);
        if let Some(parent) = target.parent() {
            std_fs::create_dir_all(parent).map_err(PluginError::Io)?;
        }
        let mut file = std_fs::File::create(&target).map_err(PluginError::Io)?;
        io::copy(&mut entry, &mut file).map_err(PluginError::Io)?;
    }
    Ok(())
}

/// Parse a `.dxt` / `.mcpb` archive from bytes and return the manifest.
///
/// Performs path-traversal sanitization but does NOT write to disk. Useful
/// for "dry-run" inspection or signature verification before install.
pub fn parse_extension_archive(bytes: &[u8]) -> PluginResult<PluginManifest> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| PluginError::InvalidManifest(format!("invalid zip: {e}")))?;
    find_manifest(&mut archive)
}

/// Install a `.dxt` / `.mcpb` archive from bytes into `dest_root`.
///
/// The archive is extracted to `<dest_root>/<manifest.name>/`. Returns the
/// resolved plugin name (from the manifest) so the caller can register it.
pub fn install_extension_bytes(
    bytes: &[u8],
    dest_root: &Path,
    kind: ExtensionKind,
) -> PluginResult<String> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| PluginError::InvalidManifest(format!("invalid {}: {e}", kind.extension())))?;

    let manifest = find_manifest(&mut archive)?;
    // Re-open the cursor for extraction since `find_manifest` borrowed it mutably.
    drop(archive);
    let cursor2 = Cursor::new(bytes);
    let mut archive2 = zip::ZipArchive::new(cursor2)
        .map_err(|e| PluginError::InvalidManifest(format!("invalid {}: {e}", kind.extension())))?;

    let target_dir = dest_root.join(sanitize_name(&manifest.name));
    extract_zip(&mut archive2, &target_dir)?;

    Ok(manifest.name.clone())
}

/// Install a `.dxt` / `.mcpb` file from a filesystem path.
///
/// Reads the file, infers the kind from the extension (falls back to `Dxt`
/// when unknown), and delegates to [`install_extension_bytes`].
pub fn install_extension_file(path: &Path, dest_root: &Path) -> PluginResult<String> {
    let bytes = std_fs::read(path).map_err(PluginError::Io)?;
    let kind = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(ExtensionKind::from_extension)
        .unwrap_or(ExtensionKind::Dxt);
    install_extension_bytes(&bytes, dest_root, kind)
}

/// Make a manifest name safe for use as a directory name.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('.')
        .to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::{Seek, Write};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        {
            let mut zw = ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            for (name, data) in entries {
                zw.start_file(*name, opts).unwrap();
                zw.write_all(data).unwrap();
            }
            zw.finish().unwrap();
        }
        buf.seek(io::SeekFrom::Start(0)).unwrap();
        buf.into_inner()
    }

    fn manifest_json(name: &str) -> Vec<u8> {
        format!(
            "{{\"name\":\"{name}\",\"version\":\"1.0.0\",\
             \"description\":\"test\",\"type\":\"skill\",\
             \"entry\":\"t.md\",\"trigger\":\"/x\",\"template\":\"hi\"}}"
        )
        .into_bytes()
    }

    #[test]
    fn extension_kind_round_trip() {
        assert_eq!(
            ExtensionKind::from_extension("dxt"),
            Some(ExtensionKind::Dxt)
        );
        assert_eq!(
            ExtensionKind::from_extension("DXT"),
            Some(ExtensionKind::Dxt)
        );
        assert_eq!(
            ExtensionKind::from_extension("mcpb"),
            Some(ExtensionKind::Mcpb)
        );
        assert_eq!(ExtensionKind::from_extension("exe"), None);
        assert_eq!(ExtensionKind::Dxt.extension(), "dxt");
    }

    #[test]
    fn parse_extension_archive_finds_root_manifest_json() {
        let bytes = build_zip(&[("manifest.json", &manifest_json("root-plugin"))]);
        let m = parse_extension_archive(&bytes).unwrap();
        assert_eq!(m.name, "root-plugin");
    }

    #[test]
    fn parse_extension_archive_finds_claude_subdir() {
        let bytes = build_zip(&[(
            ".claude-plugin/plugin.json",
            &manifest_json("claude-plugin"),
        )]);
        let m = parse_extension_archive(&bytes).unwrap();
        assert_eq!(m.name, "claude-plugin");
    }

    #[test]
    fn parse_extension_archive_finds_toml_root() {
        let toml = b"name = \"toml-plugin\"\nversion = \"1.0.0\"\ndescription = \"x\"\n\
                    type = \"skill\"\nentry = \"t.md\"\ntrigger = \"/y\"\ntemplate = \"y\"\n";
        let bytes = build_zip(&[("plugin.toml", toml)]);
        let m = parse_extension_archive(&bytes).unwrap();
        assert_eq!(m.name, "toml-plugin");
    }

    #[test]
    fn parse_extension_archive_missing_manifest_errors() {
        let bytes = build_zip(&[("README.md", b"no manifest here")]);
        let err = parse_extension_archive(&bytes).unwrap_err().to_string();
        assert!(err.contains("no manifest found"));
    }

    #[test]
    fn install_extension_bytes_extracts_files() {
        let bytes = build_zip(&[
            ("manifest.json", &manifest_json("installable")),
            ("template.md", b"hello world"),
            ("nested/deep.txt", b"nested content"),
        ]);
        let tmp = tempfile::TempDir::new().unwrap();
        let name = install_extension_bytes(&bytes, tmp.path(), ExtensionKind::Dxt).unwrap();
        assert_eq!(name, "installable");

        let base = tmp.path().join("installable");
        assert!(base.join("manifest.json").exists());
        let body = std_fs::read(base.join("template.md")).unwrap();
        assert_eq!(body, b"hello world");
        let nested = std_fs::read(base.join("nested/deep.txt")).unwrap();
        assert_eq!(nested, b"nested content");
    }

    #[test]
    fn install_extension_rejects_path_traversal() {
        // Manifest first so we reach the extraction pass; then a traversal entry.
        let mut buf: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        {
            let mut zw = ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("manifest.json", opts).unwrap();
            zw.write_all(&manifest_json("trav-test")).unwrap();
            zw.start_file("../escape.txt", opts).unwrap();
            zw.write_all(b"pwn").unwrap();
            zw.finish().unwrap();
        }
        let bytes = buf.into_inner();
        let tmp = tempfile::TempDir::new().unwrap();
        let err = install_extension_bytes(&bytes, tmp.path(), ExtensionKind::Dxt)
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("traversal"), "got: {err}");
        // Ensure the escape file was NOT created above the temp dir
        assert!(!tmp.path().join("..").join("escape.txt").exists());
    }

    #[test]
    fn sanitize_name_strips_unsafe_chars() {
        assert_eq!(sanitize_name("good-name_1"), "good-name_1");
        assert_eq!(sanitize_name("../bad"), "_bad");
        assert_eq!(sanitize_name("trailing."), "trailing");
        assert_eq!(sanitize_name("a/b\\c"), "a_b_c");
    }

    #[test]
    fn install_extension_file_infers_kind_from_extension() {
        let bytes = build_zip(&[("manifest.json", &manifest_json("from-file"))]);
        let tmp = tempfile::TempDir::new().unwrap();
        let file_path = tmp.path().join("bundle.dxt");
        std_fs::write(&file_path, &bytes).unwrap();

        let dest = tmp.path().join("dest");
        let name = install_extension_file(&file_path, &dest).unwrap();
        assert_eq!(name, "from-file");
        assert!(dest.join("from-file/manifest.json").exists());
    }
}
