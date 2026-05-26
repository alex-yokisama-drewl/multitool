//! Raster image formats that the multitool image pipeline can both **decode
//! and encode** — i.e. formats a tool can write back out without a lossy
//! cross-format conversion. The single source of truth for the picker
//! filters, the format-converter output dropdown, and the image-crop tool.
//!
//! SVG is intentionally excluded — it's decode-only here (we rasterize to
//! PNG/JPEG/etc., never write SVG). Read-only formats accepted by the
//! image-format converter (GIF/ICO/TGA/PNM/QOI/SVG) live in that tool's
//! input-picker list, not here.

use image::ImageFormat;
use serde::{Deserialize, Serialize};

/// One of the raster formats the pipeline can encode. Variants are stable;
/// adding a new format means: variant + match arm in every method below,
/// and any caller that pattern-matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RasterFormat {
    Png,
    Jpeg,
    Webp,
    Bmp,
    Tiff,
}

impl RasterFormat {
    /// All variants in stable display order. Use for IPC enumeration and
    /// dropdown population.
    pub const fn all() -> &'static [RasterFormat] {
        &[Self::Png, Self::Jpeg, Self::Webp, Self::Bmp, Self::Tiff]
    }

    /// Stable wire id (matches the `#[serde(rename_all = "lowercase")]`
    /// representation: `"png"`, `"jpeg"`, …). Spelled out so callers don't
    /// have to round-trip through `serde_json` to get a string.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
            Self::Bmp => "bmp",
            Self::Tiff => "tiff",
        }
    }

    /// Human-readable name for the UI.
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Webp => "WebP",
            Self::Bmp => "BMP",
            Self::Tiff => "TIFF",
        }
    }

    /// All accepted file extensions for this format (lowercase, no leading
    /// dot). First entry is the canonical extension used when writing output.
    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Png => &["png"],
            Self::Jpeg => &["jpg", "jpeg"],
            Self::Webp => &["webp"],
            Self::Bmp => &["bmp"],
            Self::Tiff => &["tif", "tiff"],
        }
    }

    /// Canonical extension for output-file naming.
    pub const fn default_extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Bmp => "bmp",
            Self::Tiff => "tif",
        }
    }

    /// True when the encoder preserves an alpha channel without flattening.
    /// JPEG and BMP have no alpha; PNG/WebP/TIFF do.
    pub const fn supports_alpha(self) -> bool {
        matches!(self, Self::Png | Self::Webp | Self::Tiff)
    }

    /// Mapping to `image::ImageFormat` for use with `DynamicImage::write_to`.
    pub const fn image_format(self) -> ImageFormat {
        match self {
            Self::Png => ImageFormat::Png,
            Self::Jpeg => ImageFormat::Jpeg,
            Self::Webp => ImageFormat::WebP,
            Self::Bmp => ImageFormat::Bmp,
            Self::Tiff => ImageFormat::Tiff,
        }
    }

    /// Look up a format by file extension (case-insensitive, no leading dot).
    /// Returns `None` for any extension outside the raster set — including
    /// formats the pipeline can decode but not encode (e.g. `"gif"`).
    pub fn from_extension(ext: &str) -> Option<Self> {
        Self::all()
            .iter()
            .copied()
            .find(|fmt| fmt.extensions().iter().any(|e| e.eq_ignore_ascii_case(ext)))
    }
}

/// Serializable descriptor for IPC — the shape the frontend sees. Built
/// from a `RasterFormat` via [`RasterFormat::descriptor`] (added in the
/// IPC-command commit).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RasterFormatDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub supports_alpha: bool,
}

impl RasterFormat {
    /// IPC descriptor for this format. Const-friendly: all fields are
    /// `&'static`.
    pub const fn descriptor(self) -> RasterFormatDescriptor {
        RasterFormatDescriptor {
            id: self.id(),
            name: self.display_name(),
            extensions: self.extensions(),
            supports_alpha: self.supports_alpha(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_variants_listed() {
        // Five formats today; this asserts the count + identity rather than
        // duplicating the list — adding a variant requires touching `all()`.
        let all = RasterFormat::all();
        assert_eq!(all.len(), 5);
        assert!(all.contains(&RasterFormat::Png));
        assert!(all.contains(&RasterFormat::Jpeg));
        assert!(all.contains(&RasterFormat::Webp));
        assert!(all.contains(&RasterFormat::Bmp));
        assert!(all.contains(&RasterFormat::Tiff));
    }

    #[test]
    fn ids_match_serde_lowercase() {
        // The id() shortcut MUST match what serde produces via the
        // rename_all="lowercase" attribute, or IPC payloads will desync from
        // the frontend's enum mapping.
        for fmt in RasterFormat::all() {
            let serialized = serde_json::to_string(fmt).unwrap();
            assert_eq!(serialized, format!("\"{}\"", fmt.id()));
        }
    }

    #[test]
    fn default_extension_is_first_listed() {
        for fmt in RasterFormat::all() {
            assert_eq!(fmt.default_extension(), fmt.extensions()[0]);
        }
    }

    #[test]
    fn supports_alpha_split() {
        assert!(RasterFormat::Png.supports_alpha());
        assert!(RasterFormat::Webp.supports_alpha());
        assert!(RasterFormat::Tiff.supports_alpha());
        assert!(!RasterFormat::Jpeg.supports_alpha());
        assert!(!RasterFormat::Bmp.supports_alpha());
    }

    #[test]
    fn from_extension_is_case_insensitive() {
        assert_eq!(RasterFormat::from_extension("png"), Some(RasterFormat::Png));
        assert_eq!(RasterFormat::from_extension("PNG"), Some(RasterFormat::Png));
        assert_eq!(
            RasterFormat::from_extension("jpeg"),
            Some(RasterFormat::Jpeg)
        );
        assert_eq!(
            RasterFormat::from_extension("jpg"),
            Some(RasterFormat::Jpeg)
        );
        assert_eq!(
            RasterFormat::from_extension("JPG"),
            Some(RasterFormat::Jpeg)
        );
        assert_eq!(
            RasterFormat::from_extension("tiff"),
            Some(RasterFormat::Tiff)
        );
        assert_eq!(
            RasterFormat::from_extension("tif"),
            Some(RasterFormat::Tiff)
        );
    }

    #[test]
    fn from_extension_rejects_decode_only_and_unknown() {
        // GIF/ICO/SVG are decoded by the image-format converter but not
        // emitted — they MUST NOT be selectable as an output / preserve
        // target.
        assert_eq!(RasterFormat::from_extension("gif"), None);
        assert_eq!(RasterFormat::from_extension("ico"), None);
        assert_eq!(RasterFormat::from_extension("svg"), None);
        assert_eq!(RasterFormat::from_extension(""), None);
        assert_eq!(RasterFormat::from_extension("xyz"), None);
    }

    #[test]
    fn image_format_round_trip_via_from_extension() {
        // Every variant should be reachable from its canonical extension.
        for fmt in RasterFormat::all() {
            assert_eq!(
                RasterFormat::from_extension(fmt.default_extension()),
                Some(*fmt)
            );
        }
    }

    #[test]
    fn descriptor_round_trip_serializes_stable_shape() {
        // The descriptor is the wire shape. Lock it down with a JSON
        // assertion so an accidental field rename surfaces in tests, not
        // in a broken frontend.
        let d = RasterFormat::Jpeg.descriptor();
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "jpeg",
                "name": "JPEG",
                "extensions": ["jpg", "jpeg"],
                "supports_alpha": false,
            })
        );
    }

    #[test]
    fn descriptor_list_is_built_for_every_format_with_unique_ids() {
        // Mirrors what the `supported_raster_formats` Tauri command does (the
        // shell command can't be unit-tested — see ADDING_A_TOOL §2). Asserts
        // the map produces one descriptor per format and that ids are unique,
        // so the frontend can key on `id` safely.
        let descriptors: Vec<_> = RasterFormat::all().iter().map(|f| f.descriptor()).collect();
        assert_eq!(descriptors.len(), RasterFormat::all().len());
        let unique_ids: std::collections::HashSet<_> = descriptors.iter().map(|d| d.id).collect();
        assert_eq!(unique_ids.len(), descriptors.len());
    }
}
