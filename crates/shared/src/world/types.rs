use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InsideKind {
    Generated,
    Ephemeral,
    Permanent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AreaKind {
    Outside,
    Inside(InsideKind),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AreaMetadata {
    pub id: String,
    pub name: String,
    pub kind: AreaKind,
    /// `(width, length)` in meters for fixed Inside areas; `None` for Outside or Generated areas.
    pub dimensions: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropPlacement {
    pub id: u64,
    pub asset_id: String,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    pub cast_shadow: bool,
}

pub(crate) fn decode_exact<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, postcard::Error> {
    let (value, rest) = postcard::take_from_bytes::<T>(bytes)?;
    if !rest.is_empty() {
        return Err(postcard::Error::SerdeDeCustom);
    }
    Ok(value)
}

/// Prefix used in `PropPlacement::asset_id` for one-off `.glb` decorations scoped to a single area
/// (`world/areas/<area_id>/assets/<slug>.glb`) rather than the global `catalog.json`.
pub const LOCAL_ASSET_PREFIX: &str = "local:";

/// Returns `true` when `slug` is a safe 1–64 char ASCII `[a-z0-9_-]` asset identifier.
pub fn is_valid_asset_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 64
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

#[inline]
pub fn is_local_asset_id(asset_id: &str) -> bool {
    asset_id.starts_with(LOCAL_ASSET_PREFIX)
}

/// Extracts and validates `<slug>` from a `"local:<slug>"` `asset_id`.
pub fn local_asset_slug(asset_id: &str) -> Option<&str> {
    let slug = asset_id.strip_prefix(LOCAL_ASSET_PREFIX)?;
    is_valid_asset_slug(slug).then_some(slug)
}

#[inline]
pub fn format_local_asset_id(slug: &str) -> String {
    format!("{LOCAL_ASSET_PREFIX}{slug}")
}

#[inline]
pub fn local_asset_http_path(area_id: &str, slug: &str) -> String {
    format!("/world/areas/{area_id}/assets/{slug}.glb")
}

/// Sanitizes an arbitrary filename or label (e.g. `"Hero Cryo-Pod (v2).glb"`) into a valid asset slug.
pub fn sanitize_asset_slug(raw: &str) -> String {
    let base = raw.trim().rsplit(['/', '\\']).next().unwrap_or(raw.trim());
    let stem = base
        .strip_suffix(".glb")
        .or_else(|| base.strip_suffix(".GLB"))
        .unwrap_or(base);

    let mut out = String::with_capacity(stem.len().min(64));
    let mut prev_sep = false;
    for ch in stem.chars() {
        if out.len() >= 64 {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if ch == '-' && !out.is_empty() && !prev_sep {
            out.push('-');
            prev_sep = true;
        } else if !out.is_empty() && !prev_sep {
            out.push('_');
            prev_sep = true;
        }
    }
    while out.ends_with(['_', '-']) {
        out.pop();
    }
    if out.is_empty() {
        "imported_prop".to_owned()
    } else {
        out
    }
}

/// Formats an asset slug such as `"hero_cryo-pod"` into a human-friendly label (`"Hero Cryo Pod"`).
pub fn prettify_asset_slug(slug: &str) -> String {
    let words: Vec<String> = slug
        .split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut w = String::with_capacity(part.len());
                    w.push(first.to_ascii_uppercase());
                    w.push_str(chars.as_str());
                    w
                }
                None => String::new(),
            }
        })
        .collect();
    if words.is_empty() {
        slug.to_owned()
    } else {
        words.join(" ")
    }
}

/// Catalog entry for a `.glb` decoration or prop asset served over HTTP(S).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub category: String,
}

/// Catalog entry for a terrain splat material texture pair served over HTTP(S).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerrainTextureEntry {
    pub layer: u8,
    pub id: String,
    pub name: String,
    pub category: String,
    pub albedo_path: String,
    #[serde(default)]
    pub height_path: Option<String>,
}

fn default_road_edge() -> String {
    "Curbed".to_owned()
}

/// Catalog entry for a road ribbon RGBA texture served over HTTP(S).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoadTextureEntry {
    pub layer: u8,
    pub id: String,
    pub name: String,
    #[serde(default = "default_road_edge")]
    pub edge: String,
    pub path: String,
}

impl RoadTextureEntry {
    #[inline]
    pub fn is_soft_edge(&self) -> bool {
        self.edge.eq_ignore_ascii_case("soft")
    }
}

/// Unified asset catalog served from `/world/assets/catalog.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetCatalog {
    #[serde(default)]
    pub decorations: Vec<AssetEntry>,
    #[serde(default)]
    pub terrain_textures: Vec<TerrainTextureEntry>,
    #[serde(default)]
    pub road_textures: Vec<RoadTextureEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_asset_slug_sanitizes_and_rejects_traversal() {
        assert_eq!(
            sanitize_asset_slug("C:\\exports\\Hero Cryo-Pod (v2).GLB"),
            "hero_cryo-pod_v2"
        );
        assert_eq!(sanitize_asset_slug("  ...  "), "imported_prop");
        assert_eq!(prettify_asset_slug("hero_cryo-pod"), "Hero Cryo Pod");

        let id = format_local_asset_id("cryo_capsule");
        assert!(is_local_asset_id(&id));
        assert_eq!(local_asset_slug(&id), Some("cryo_capsule"));
        assert_eq!(
            local_asset_http_path("cryo_prison", "cryo_capsule"),
            "/world/areas/cryo_prison/assets/cryo_capsule.glb"
        );

        assert_eq!(local_asset_slug("pine_tree"), None);
        assert_eq!(local_asset_slug("local:"), None);
        assert_eq!(local_asset_slug("local:../secret"), None);
        assert_eq!(local_asset_slug("local:UpperCase"), None);
    }
}
