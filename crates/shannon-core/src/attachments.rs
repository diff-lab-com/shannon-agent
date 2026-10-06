//! Shared attachment validation — the single source of truth for the
//! multimodal attachment rules.
//!
//! Every entry path that accepts attachments (REST `/v1/sessions/:id/messages`
//! and `/api/query`, the desktop `send_message`, the TUI `@`-reference image
//! path, and the headless `--attach` flag) enforces the same limits through
//! these helpers, so one change here updates all of them.
//!
//! Size limits are checked in two stages:
//! 1. **Before decoding** — [`validate_base64_size`] estimates the decoded
//!    size from the base64 character length, so an oversized payload is
//!    rejected without ever materialising the bytes in memory (the old
//!    decode-then-compare order paid a ~4/3 memory amplification per
//!    attachment).
//! 2. **After decoding** — [`validate_decoded_size`] makes the exact call on
//!    the real bytes; the estimate is deliberately padded so it can never
//!    reject a payload whose decoded size is within the limit.

/// Maximum attachments per message (Anthropic accepts up to 100; this keeps
/// a single request's multimodal payload bounded).
pub const MAX_ATTACHMENTS: usize = 8;

/// 10 MiB per image attachment after base64 decode.
pub const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// Hard cap on PDF attachments (metadata precheck before any read or
/// `pdftotext` spawn). PDFs ride the text-extraction path rather than the
/// 10 MiB image path, so they get their own — larger — ceiling.
pub const MAX_PDF_BYTES: u64 = 100 * 1024 * 1024;

/// MIME types the multimodal adapters can serialize (vision providers
/// accept png/jpeg/gif/webp only).
pub const SUPPORTED_IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// Whether `media_type` is accepted on the multimodal (base64 image block)
/// path. Exact, case-sensitive match — mirrors the historical REST
/// allowlist.
pub fn is_supported_image_type(media_type: &str) -> bool {
    SUPPORTED_IMAGE_TYPES.contains(&media_type)
}

/// Why an attachment was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentError {
    /// More than [`MAX_ATTACHMENTS`] in one message.
    TooMany { count: usize, max: usize },
    /// Payload over the size limit (`size` may be a conservative estimate
    /// when it comes from the pre-decode check).
    TooLarge { size: usize, max: usize },
    /// Media type outside [`SUPPORTED_IMAGE_TYPES`].
    Unsupported { media_type: String },
}

impl std::fmt::Display for AttachmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooMany { count, max } => {
                write!(f, "too many attachments: {count} (max {max})")
            }
            Self::TooLarge { size, max } => {
                write!(f, "{size} bytes exceeds the {max} byte limit")
            }
            Self::Unsupported { media_type } => write!(
                f,
                "unsupported media_type \"{media_type}\" (supported: {})",
                SUPPORTED_IMAGE_TYPES.join(", ")
            ),
        }
    }
}

impl std::error::Error for AttachmentError {}

/// Reject message-level attachment count violations (`n <= MAX_ATTACHMENTS`).
pub fn validate_count(n: usize) -> Result<(), AttachmentError> {
    if n > MAX_ATTACHMENTS {
        return Err(AttachmentError::TooMany {
            count: n,
            max: MAX_ATTACHMENTS,
        });
    }
    Ok(())
}

/// Pre-decode size gate: estimate the decoded length from the base64
/// character count (`decoded ≈ raw_len * 3 / 4`) and reject oversized
/// payloads before any bytes are decoded.
///
/// The estimate pads the raw length by the up-to-two base64 padding
/// characters, so a payload whose decoded size is exactly [`MAX_IMAGE_BYTES`]
/// always passes; the exact post-decode check ([`validate_decoded_size`])
/// still catches anything that slips past the estimate.
pub fn validate_base64_size(raw_len: usize) -> Result<(), AttachmentError> {
    let estimated = raw_len.saturating_sub(2) / 4 * 3;
    if estimated > MAX_IMAGE_BYTES {
        return Err(AttachmentError::TooLarge {
            size: estimated,
            max: MAX_IMAGE_BYTES,
        });
    }
    Ok(())
}

/// Exact post-decode size gate (`len <= MAX_IMAGE_BYTES`).
pub fn validate_decoded_size(len: usize) -> Result<(), AttachmentError> {
    if len > MAX_IMAGE_BYTES {
        return Err(AttachmentError::TooLarge {
            size: len,
            max: MAX_IMAGE_BYTES,
        });
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn count_at_limit_passes_one_over_fails() {
        assert_eq!(validate_count(MAX_ATTACHMENTS), Ok(()));
        assert_eq!(validate_count(0), Ok(()));
        let err = validate_count(MAX_ATTACHMENTS + 1).expect_err("count over the limit must fail");
        assert!(
            matches!(err, AttachmentError::TooMany { count, max } if count == MAX_ATTACHMENTS + 1 && max == MAX_ATTACHMENTS)
        );
    }

    #[test]
    fn decoded_size_at_limit_passes_one_over_fails() {
        assert_eq!(validate_decoded_size(0), Ok(()));
        assert_eq!(validate_decoded_size(MAX_IMAGE_BYTES), Ok(()));
        let err =
            validate_decoded_size(MAX_IMAGE_BYTES + 1).expect_err("decoded size over the limit");
        assert!(
            matches!(err, AttachmentError::TooLarge { size, max } if size == MAX_IMAGE_BYTES + 1 && max == MAX_IMAGE_BYTES)
        );
        assert_eq!(
            err.to_string(),
            format!(
                "{} bytes exceeds the {MAX_IMAGE_BYTES} byte limit",
                MAX_IMAGE_BYTES + 1
            )
        );
    }

    #[test]
    fn base64_estimate_never_rejects_payload_at_limit() {
        // A payload decoding to exactly the limit encodes to
        // ceil(MAX/3)*4 base64 characters (padding included). The estimate
        // must let it (and one padding-width neighbour) through — the
        // post-decode check is the exact gate.
        let raw_len = MAX_IMAGE_BYTES.div_ceil(3) * 4;
        assert_eq!(validate_base64_size(raw_len), Ok(()));
        assert_eq!(validate_base64_size(raw_len + 1), Ok(()));
    }

    #[test]
    fn base64_estimate_rejects_oversized_length_without_decoding() {
        let raw_len = MAX_IMAGE_BYTES.div_ceil(3) * 4;
        // Two characters later the padded estimate crosses the limit and
        // the pre-check rejects without any decoding.
        assert!(validate_base64_size(raw_len + 2).is_err());
        // Far over the limit is rejected too.
        let err = validate_base64_size(64 * 1024 * 1024).expect_err("64 MiB of base64 must fail");
        assert!(matches!(err, AttachmentError::TooLarge { .. }));
    }

    #[test]
    fn supported_image_types_match_rest_allowlist() {
        for mt in SUPPORTED_IMAGE_TYPES {
            assert!(is_supported_image_type(mt), "{mt} must be supported");
        }
        for mt in [
            "",
            "image/svg+xml",
            "image/bmp",
            "application/pdf",
            "image/PNG",
        ] {
            assert!(!is_supported_image_type(mt), "{mt} must be rejected");
        }
    }

    #[test]
    fn error_display_keeps_entry_path_wording() {
        assert_eq!(
            AttachmentError::TooMany { count: 9, max: 8 }.to_string(),
            "too many attachments: 9 (max 8)"
        );
        assert_eq!(
            AttachmentError::TooLarge { size: 11, max: 10 }.to_string(),
            "11 bytes exceeds the 10 byte limit"
        );
        assert_eq!(
            AttachmentError::Unsupported {
                media_type: "image/svg+xml".to_string()
            }
            .to_string(),
            "unsupported media_type \"image/svg+xml\" (supported: image/png, image/jpeg, image/gif, image/webp)"
        );
    }
}
