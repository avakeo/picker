//! The 12-color search palette (design 9.4, ADR-006).
//!
//! An extracted color belongs to the palette color nearest to it in CIELAB,
//! provided that the distance is within the threshold. A color with little
//! chroma (gray) is compared with white and black only, so it falls to the
//! nearer of the two instead of a dull chromatic color such as brown.

use picker_imaging::{delta_e, rgb_to_lab};
use serde::{Deserialize, Serialize};

use crate::library::{LibraryError, Result};

const BUNDLED: &str = include_str!("../resources/palette.json");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaletteColor {
    pub id: String,
    pub name_ja: String,
    pub hex: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PaletteFile {
    #[allow(dead_code)]
    status: String,
    #[allow(dead_code)]
    note: String,
    threshold: f64,
    achromatic: AchromaticRule,
    colors: Vec<PaletteColor>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AchromaticRule {
    max_chroma: f64,
    light_id: String,
    dark_id: String,
}

/// Which extracted colors a color condition looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorScope {
    /// Only the main (largest) color.
    Main,
    /// Any of the up to three extracted colors.
    AnyOfThree,
}

impl ColorScope {
    #[must_use]
    pub const fn max_rank(self) -> u8 {
        match self {
            Self::Main => 1,
            Self::AnyOfThree => 3,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Palette {
    threshold: f64,
    max_chroma: f64,
    /// Indexes of the white and black entries.
    achromatic: [usize; 2],
    colors: Vec<PaletteColor>,
    labs: Vec<[f64; 3]>,
}

pub(crate) fn parse_hex(value: &str) -> Option<[u8; 3]> {
    let digits = value.strip_prefix('#')?;
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let channel = |index: usize| u8::from_str_radix(&digits[index..index + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

impl Palette {
    /// The palette bundled with the app, parsed once.
    pub fn bundled() -> Result<&'static Self> {
        static PALETTE: std::sync::OnceLock<std::result::Result<Palette, String>> =
            std::sync::OnceLock::new();
        PALETTE
            .get_or_init(|| Self::from_json(BUNDLED).map_err(|error| error.to_string()))
            .as_ref()
            .map_err(|error| LibraryError::InvalidInput(error.clone()))
    }

    /// Parses a palette file. Ids must be unique and colors valid `#RRGGBB`.
    pub fn from_json(json: &str) -> Result<Self> {
        let file: PaletteFile = serde_json::from_str(json)
            .map_err(|error| LibraryError::InvalidInput(format!("palette: {error}")))?;
        if file.colors.is_empty() || !file.threshold.is_finite() || file.threshold <= 0.0 {
            return Err(LibraryError::InvalidInput(
                "palette needs colors and a positive threshold".to_owned(),
            ));
        }
        let mut labs = Vec::with_capacity(file.colors.len());
        for (index, color) in file.colors.iter().enumerate() {
            if file.colors[..index]
                .iter()
                .any(|other| other.id == color.id)
            {
                return Err(LibraryError::InvalidInput(format!(
                    "duplicate palette id: {}",
                    color.id
                )));
            }
            let rgb = parse_hex(&color.hex).ok_or_else(|| {
                LibraryError::InvalidInput(format!("invalid palette color: {}", color.hex))
            })?;
            labs.push(rgb_to_lab(rgb));
        }
        let index_of = |id: &str| {
            file.colors
                .iter()
                .position(|color| color.id == id)
                .ok_or_else(|| LibraryError::InvalidInput(format!("unknown palette id: {id}")))
        };
        let achromatic = [
            index_of(&file.achromatic.light_id)?,
            index_of(&file.achromatic.dark_id)?,
        ];
        if !file.achromatic.max_chroma.is_finite() || file.achromatic.max_chroma < 0.0 {
            return Err(LibraryError::InvalidInput(
                "palette maxChroma must not be negative".to_owned(),
            ));
        }
        Ok(Self {
            threshold: file.threshold,
            max_chroma: file.achromatic.max_chroma,
            achromatic,
            colors: file.colors,
            labs,
        })
    }

    #[must_use]
    pub fn colors(&self) -> &[PaletteColor] {
        &self.colors
    }

    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.colors.iter().any(|color| color.id == id)
    }

    /// Returns the palette color id an extracted color belongs to, if any.
    #[must_use]
    pub fn classify(&self, hex: &str) -> Option<&str> {
        let lab = rgb_to_lab(parse_hex(hex)?);
        let chroma = lab[1].hypot(lab[2]);
        let candidates: Vec<usize> = if chroma <= self.max_chroma {
            self.achromatic.to_vec()
        } else {
            (0..self.labs.len()).collect()
        };
        let (index, distance) = candidates
            .into_iter()
            .map(|index| (index, delta_e(lab, self.labs[index])))
            .min_by(|left, right| left.1.total_cmp(&right.1))?;
        (distance <= self.threshold).then(|| self.colors[index].id.as_str())
    }
}

/// A color condition of a search: images having any of the selected palette
/// colors within the scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorCondition {
    pub palette_ids: Vec<String>,
    pub scope: ColorScope,
}

impl crate::Library {
    /// Narrows `filter` to images matching the color condition. An empty
    /// condition leaves the filter unchanged.
    pub fn restrict_to_colors(
        &self,
        filter: &mut picker_store::AssetFilter,
        condition: &ColorCondition,
    ) -> Result<()> {
        if condition.palette_ids.is_empty() {
            return Ok(());
        }
        let palette = Palette::bundled()?;
        if let Some(unknown) = condition
            .palette_ids
            .iter()
            .find(|id| !palette.contains(id))
        {
            return Err(LibraryError::InvalidInput(format!(
                "unknown palette color: {unknown}"
            )));
        }
        let mut matched: Vec<i64> = self
            .store()
            .colors()
            .colors_up_to_rank(condition.scope.max_rank())?
            .into_iter()
            .filter(|(_, hex)| {
                palette
                    .classify(hex)
                    .is_some_and(|id| condition.palette_ids.iter().any(|wanted| wanted == id))
            })
            .map(|(asset_id, _)| asset_id)
            .collect();
        matched.dedup();
        filter.asset_ids = Some(match filter.asset_ids.take() {
            Some(existing) => {
                let existing: std::collections::HashSet<i64> = existing.into_iter().collect();
                matched
                    .into_iter()
                    .filter(|id| existing.contains(id))
                    .collect()
            }
            None => matched,
        });
        Ok(())
    }
}
