//! `.mcpb` bundle extraction (ZIP + manifest.json).
//!
//! `.mcpb` is the one-click install format defined by
//! `modelcontextprotocol/mcpb`. A `.mcpb` is a ZIP archive containing:
//!
//! ```text
//! my-server.mcpb
//! ├── manifest.json    # name, version, server config, optional icon
//! ├── README.md
//! └── src/             # actual server code (Node, Python, etc.)
//! ```
//!
//! The installer downloads the file, verifies the manifest, extracts it to
//! `~/.shannon/mcp-servers/<name>/`, and writes an `mcpServers` entry pointing
//! at the extracted entrypoint.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::installer::InstallError;

/// Parsed `.mcpb/manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbManifest {
    /// `manifest_version` per spec — currently "0.1" or "0.2".
    #[serde(rename = "manifest_version")]
    pub manifest_version: String,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Server definition — varies by transport.
    pub server: McpbServer,
    #[serde(default)]
    pub author: Option<McpbAuthor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbServer {
    /// `stdio`, `sse`, or `http`.
    #[serde(rename = "type")]
    pub server_type: String,
    /// For stdio: the entrypoint command.
    #[serde(default)]
    pub command: Option<String>,
    /// For stdio: argv.
    #[serde(default)]
    pub args: Vec<String>,
    /// For stdio: env vars.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// For sse/http: the URL.
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbAuthor {
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
}

/// Extract a `.mcpb` archive from bytes into the target directory.
///
/// The target directory will be created if missing. Returns the parsed
/// manifest and the path the caller should record as the install path.
pub fn extract_mcpb(
    bytes: &[u8],
    target_dir: &Path,
) -> Result<(McpbManifest, PathBuf), InstallError> {
    let manifest = parse_manifest_from_zip(bytes)?;

    let server_root = target_dir.join(&manifest.name);
    std::fs::create_dir_all(&server_root).map_err(|e| InstallError::Io(e.to_string()))?;

    let cursor = Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| InstallError::Format(format!("zip read: {e}")))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| InstallError::Format(format!("zip entry {i}: {e}")))?;
        let entry_name = entry.name().to_string();

        // Defense against path traversal — entry names like `../../etc/passwd`.
        let safe_path = sanitize_zip_path(&entry_name, &server_root)?;

        if entry.is_dir() {
            std::fs::create_dir_all(&safe_path).map_err(|e| InstallError::Io(e.to_string()))?;
        } else {
            if let Some(parent) = safe_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| InstallError::Io(e.to_string()))?;
            }
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry
                .read_to_end(&mut buf)
                .map_err(|e| InstallError::Io(format!("zip read body: {e}")))?;
            std::fs::write(&safe_path, &buf).map_err(|e| InstallError::Io(e.to_string()))?;
        }
    }

    Ok((manifest, server_root))
}

fn parse_manifest_from_zip(bytes: &[u8]) -> Result<McpbManifest, InstallError> {
    let cursor = Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| InstallError::Format(format!("zip read: {e}")))?;

    // Search for manifest.json at archive root (not nested in subdirectory).
    let mut manifest_idx: Option<usize> = None;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| InstallError::Format(format!("zip entry {i}: {e}")))?;
        let name = entry.name();
        if name == "manifest.json" || name.ends_with("/manifest.json") {
            manifest_idx = Some(i);
            break;
        }
    }

    let idx = manifest_idx
        .ok_or_else(|| InstallError::Format("manifest.json not found in archive".into()))?;
    let mut entry = archive
        .by_index(idx)
        .map_err(|e| InstallError::Format(format!("manifest entry: {e}")))?;

    let mut body = String::new();
    entry
        .read_to_string(&mut body)
        .map_err(|e| InstallError::Io(format!("manifest read: {e}")))?;

    let manifest: McpbManifest = serde_json::from_str(&body)
        .map_err(|e| InstallError::Format(format!("manifest parse: {e}")))?;

    if manifest.manifest_version.is_empty() {
        return Err(InstallError::Format("manifest_version missing".into()));
    }
    if manifest.name.is_empty() {
        return Err(InstallError::Format("manifest name missing".into()));
    }

    Ok(manifest)
}

/// Reject paths that escape the target directory after join.
///
/// Zip-slip attacks use entries like `../../../etc/passwd` to write outside
/// the intended root. We canonicalize and verify the result is inside `root`.
fn sanitize_zip_path(entry_name: &str, root: &Path) -> Result<PathBuf, InstallError> {
    // Replace backslashes (Windows zips) with forward slashes for normalization.
    let normalized = entry_name.replace('\\', "/");
    let entry_path = Path::new(&normalized);

    // Walk the ENTRY's components first (not root.join(entry)) so an absolute
    // root doesn't trip the RootDir check. Reject any prefix/absolute/.. parts.
    let mut composed = PathBuf::new();
    for component in entry_path.components() {
        use std::path::Component::*;
        match component {
            Prefix(_) | RootDir => {
                return Err(InstallError::Format(format!(
                    "zip entry {entry_name:?} contains absolute path"
                )));
            }
            CurDir => {}
            ParentDir => {
                return Err(InstallError::Format(format!(
                    "zip entry {entry_name:?} contains '..' (zip slip attempt)"
                )));
            }
            Normal(p) => composed.push(p),
        }
    }

    if composed.as_os_str().is_empty() {
        return Err(InstallError::Format(format!(
            "zip entry {entry_name:?} has empty path"
        )));
    }

    Ok(root.join(composed))
}

/// Compute the SHA-256 hash of the archive bytes — used by signature
/// verification (P6 work). Returned as hex.
pub fn archive_sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    hex_encode(&digest)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    fn make_minimal_mcpb(name: &str, server_type: &str, command: &str) -> Vec<u8> {
        let buf: std::io::Cursor<Vec<u8>> = std::io::Cursor::new(Vec::new());
        let mut zw = ZipWriter::new(buf);
        let opts = SimpleFileOptions::default();

        let manifest = format!(
            r#"{{
                "manifest_version": "0.1",
                "name": "{name}",
                "version": "1.0.0",
                "description": "test bundle",
                "server": {{
                    "type": "{server_type}",
                    "command": "{command}",
                    "args": []
                }}
            }}"#
        );
        zw.start_file("manifest.json", opts).unwrap();
        zw.write_all(manifest.as_bytes()).unwrap();

        zw.start_file("README.md", opts).unwrap();
        zw.write_all(b"# test").unwrap();

        zw.start_file("src/index.js", opts).unwrap();
        zw.write_all(b"console.log('hi')").unwrap();

        let buf = zw.finish().unwrap();
        buf.into_inner()
    }

    #[test]
    fn extracts_valid_mcpb() {
        let bytes = make_minimal_mcpb("my-server", "stdio", "node");
        let dir = tempdir().unwrap();
        let (manifest, server_root) = extract_mcpb(&bytes, dir.path()).expect("extract");
        assert_eq!(manifest.name, "my-server");
        assert_eq!(manifest.version.as_deref(), Some("1.0.0"));
        assert!(server_root.ends_with("my-server"));
        assert!(server_root.join("manifest.json").exists());
        assert!(server_root.join("README.md").exists());
        assert!(server_root.join("src/index.js").exists());
    }

    #[test]
    fn manifest_with_nested_subdir_is_found() {
        let buf: std::io::Cursor<Vec<u8>> = std::io::Cursor::new(Vec::new());
        let mut zw = ZipWriter::new(buf);
        let opts = SimpleFileOptions::default();
        zw.start_file("bundle/manifest.json", opts).unwrap();
        zw.write_all(
            br#"{"manifest_version":"0.1","name":"bundled","server":{"type":"stdio","command":"node"}}"#,
        )
        .unwrap();
        let bytes = zw.finish().unwrap().into_inner();

        let dir = tempdir().unwrap();
        let (manifest, _) = extract_mcpb(&bytes, dir.path()).expect("extract");
        assert_eq!(manifest.name, "bundled");
    }

    #[test]
    fn missing_manifest_errors() {
        let buf: std::io::Cursor<Vec<u8>> = std::io::Cursor::new(Vec::new());
        let mut zw = ZipWriter::new(buf);
        let opts = SimpleFileOptions::default();
        zw.start_file("README.md", opts).unwrap();
        zw.write_all(b"hello").unwrap();
        let bytes = zw.finish().unwrap().into_inner();

        let dir = tempdir().unwrap();
        let err = extract_mcpb(&bytes, dir.path()).unwrap_err();
        assert!(
            matches!(err, InstallError::Format(ref m) if m.contains("manifest.json not found"))
        );
    }

    #[test]
    fn invalid_manifest_json_errors() {
        let buf: std::io::Cursor<Vec<u8>> = std::io::Cursor::new(Vec::new());
        let mut zw = ZipWriter::new(buf);
        let opts = SimpleFileOptions::default();
        zw.start_file("manifest.json", opts).unwrap();
        zw.write_all(b"not valid json").unwrap();
        let bytes = zw.finish().unwrap().into_inner();

        let dir = tempdir().unwrap();
        let err = extract_mcpb(&bytes, dir.path()).unwrap_err();
        assert!(matches!(err, InstallError::Format(ref m) if m.contains("manifest parse")));
    }

    #[test]
    fn zip_slip_attempt_is_rejected() {
        let buf: std::io::Cursor<Vec<u8>> = std::io::Cursor::new(Vec::new());
        let mut zw = ZipWriter::new(buf);
        let opts = SimpleFileOptions::default();
        // The malicious entry — must come first so sanitize runs.
        zw.start_file("../escaped.txt", opts).unwrap();
        zw.write_all(b"pwned").unwrap();
        // Then a valid manifest so parsing succeeds.
        zw.start_file("manifest.json", opts).unwrap();
        zw.write_all(
            br#"{"manifest_version":"0.1","name":"evil","server":{"type":"stdio","command":"x"}}"#,
        )
        .unwrap();
        let bytes = zw.finish().unwrap().into_inner();

        let dir = tempdir().unwrap();
        let err = extract_mcpb(&bytes, dir.path()).unwrap_err();
        assert!(
            matches!(err, InstallError::Format(ref m) if m.contains("zip slip")),
            "got: {err:?}"
        );
    }

    #[test]
    fn absolute_path_in_zip_is_rejected() {
        let buf: std::io::Cursor<Vec<u8>> = std::io::Cursor::new(Vec::new());
        let mut zw = ZipWriter::new(buf);
        let opts = SimpleFileOptions::default();
        zw.start_file("/etc/passwd", opts).unwrap();
        zw.write_all(b"bad").unwrap();
        zw.start_file("manifest.json", opts).unwrap();
        zw.write_all(
            br#"{"manifest_version":"0.1","name":"x","server":{"type":"stdio","command":"x"}}"#,
        )
        .unwrap();
        let bytes = zw.finish().unwrap().into_inner();

        let dir = tempdir().unwrap();
        let err = extract_mcpb(&bytes, dir.path()).unwrap_err();
        assert!(
            matches!(err, InstallError::Format(ref m) if m.contains("absolute path")),
            "got: {err:?}"
        );
    }

    #[test]
    fn manifest_missing_name_errors() {
        let buf: std::io::Cursor<Vec<u8>> = std::io::Cursor::new(Vec::new());
        let mut zw = ZipWriter::new(buf);
        let opts = SimpleFileOptions::default();
        zw.start_file("manifest.json", opts).unwrap();
        zw.write_all(
            br#"{"manifest_version":"0.1","name":"","server":{"type":"stdio","command":"x"}}"#,
        )
        .unwrap();
        let bytes = zw.finish().unwrap().into_inner();

        let dir = tempdir().unwrap();
        let err = extract_mcpb(&bytes, dir.path()).unwrap_err();
        assert!(matches!(err, InstallError::Format(ref m) if m.contains("manifest name")));
    }

    #[test]
    fn manifest_missing_version_errors() {
        let buf: std::io::Cursor<Vec<u8>> = std::io::Cursor::new(Vec::new());
        let mut zw = ZipWriter::new(buf);
        let opts = SimpleFileOptions::default();
        zw.start_file("manifest.json", opts).unwrap();
        zw.write_all(
            br#"{"manifest_version":"","name":"x","server":{"type":"stdio","command":"x"}}"#,
        )
        .unwrap();
        let bytes = zw.finish().unwrap().into_inner();

        let dir = tempdir().unwrap();
        let err = extract_mcpb(&bytes, dir.path()).unwrap_err();
        assert!(matches!(err, InstallError::Format(ref m) if m.contains("manifest_version")));
    }

    #[test]
    fn sha256_returns_hex_string() {
        let bytes = b"hello";
        let hash = archive_sha256_hex(bytes);
        assert_eq!(hash.len(), 64); // 32 bytes × 2 hex chars
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sha256_known_vector() {
        // SHA-256("hello") known value.
        let hash = archive_sha256_hex(b"hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn sanitize_path_accepts_normal_relative() {
        let root = Path::new("/tmp/server");
        let p = sanitize_zip_path("src/index.js", root).unwrap();
        assert_eq!(p, Path::new("/tmp/server/src/index.js"));
    }

    #[test]
    fn sanitize_path_rejects_parent_dir() {
        let root = Path::new("/tmp/server");
        let err = sanitize_zip_path("../escape.txt", root).unwrap_err();
        assert!(matches!(err, InstallError::Format(_)));
    }
}
