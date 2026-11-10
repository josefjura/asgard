//! A Batch's Recipe: a BeerXML file exported from Brewfather, BeerSmith or similar, and the
//! light summary of it the Recipe viewer shows.
//!
//! This is the adapter over the `beerxml` reference crate, which does all the reading: encodings,
//! numbers with units in them, entities and hostile input. Here its first Recipe is mapped into
//! Asgard's own types, in metric units and integers, so the viewer and the Batch's numbers line
//! up without conversion. The crate's warnings about what it repaired are ignored.
//!
//! The file is kept as it was uploaded, byte for byte, and read again on every view. The crate
//! works out the encoding from the bytes (a UTF-8 byte order mark, then the XML declaration), so
//! transcoding the bytes would garble names like "Únětický Vzor" in an ISO-8859-1 file.

use std::fmt;

use beerxml::units::{Celsius as CrateCelsius, Kilograms, SpecificGravity};

use super::numbers::{Gravity, Percent, round_half_up};

/// The largest Recipe file accepted, in bytes: 1 MB. Real exports are a few tens of KB.
pub const RECIPE_MAX_BYTES: usize = 1024 * 1024;

/// A BeerXML file a Member uploaded, checked to hold a Recipe. It keeps the bytes as they came.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    bytes: Vec<u8>,
    summary: RecipeSummary,
}

impl Recipe {
    /// Reads `bytes` as a BeerXML file and checks it holds a Recipe. Only the first of several
    /// Recipes is summarised; the bytes are kept whole.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError::TooBig`] over [`RECIPE_MAX_BYTES`], [`RecipeError::NotBeerXml`] if
    /// the crate can't read it at all, or [`RecipeError::NoRecipe`] if it reads but holds no
    /// Recipe, such as a hops-only library export.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, RecipeError> {
        if bytes.len() > RECIPE_MAX_BYTES {
            return Err(RecipeError::TooBig);
        }
        let summary = summarise(&bytes)?;
        Ok(Self { bytes, summary })
    }

    /// The file as it was uploaded.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn summary(&self) -> &RecipeSummary {
        &self.summary
    }
}

/// Why an uploaded file can't be a Batch's Recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RecipeError {
    #[error("the file is over {RECIPE_MAX_BYTES} bytes")]
    TooBig,
    #[error("the file isn't BeerXML")]
    NotBeerXml,
    #[error("the BeerXML file holds no Recipe")]
    NoRecipe,
}

/// A Recipe as it is stored with its Batch: read again on every view. A file that read when it
/// was uploaded but not now, say after a change to the crate, still shows that a Recipe is
/// attached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachedRecipe {
    Readable(Box<RecipeSummary>),
    Unreadable,
}

impl AttachedRecipe {
    /// The stored `bytes`, read as they were uploaded.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Self {
        summarise(bytes).map_or(Self::Unreadable, |summary| {
            Self::Readable(Box::new(summary))
        })
    }

    /// The Recipe's targets, if it reads.
    #[must_use]
    pub const fn targets(&self) -> Option<&Targets> {
        match self {
            Self::Readable(summary) => Some(&summary.targets),
            Self::Unreadable => None,
        }
    }
}

/// What the Recipe viewer shows of a Recipe. Everything but the name is optional, and a list
/// may be empty: the viewer leaves out whatever the file doesn't include.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeSummary {
    pub name: String,
    /// All Grain, Partial Mash or Extract, or whatever else the file says.
    pub kind: Option<String>,
    pub style: Option<StyleSummary>,
    pub batch_size: Option<Litres>,
    pub boil_time: Option<Minutes>,
    pub efficiency: Option<Percent>,
    pub targets: Targets,
    pub fermentables: Vec<FermentableLine>,
    pub hops: Vec<HopLine>,
    pub yeasts: Vec<YeastLine>,
    pub mash_steps: Vec<MashStepLine>,
}

/// What a Recipe aims for. A target gravity outside the range a measured one can be is dropped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Targets {
    pub og: Option<Gravity>,
    pub fg: Option<Gravity>,
    pub abv: Option<Percent>,
    pub ibu: Option<Ibu>,
    pub colour: Option<Ebc>,
}

/// The style a Recipe is brewed to, with the ranges the guide sets where the file has them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleSummary {
    pub name: String,
    pub category: Option<String>,
    pub guide: Option<String>,
    pub og: Option<Range<Gravity>>,
    pub fg: Option<Range<Gravity>>,
    pub ibu: Option<Range<Ibu>>,
    pub colour: Option<Range<Ebc>>,
}

/// A range from a style guide, such as an OG of `1.044–1.060`. Only kept when both ends are in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range<T> {
    pub min: T,
    pub max: T,
}

impl<T> Range<T> {
    fn of(min: Option<T>, max: Option<T>) -> Option<Self> {
        Some(Self {
            min: min?,
            max: max?,
        })
    }
}

/// A fermentable in the grist, with its share of the grist's total weight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FermentableLine {
    pub name: String,
    pub amount: Option<Mass>,
    /// Its share of every fermentable that has an amount.
    pub share: Option<Percent>,
}

/// A hop addition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HopLine {
    pub name: String,
    pub amount: Option<Mass>,
    pub alpha: Option<Percent>,
    /// Boil, Dry Hop, Mash, First Wort, Aroma, or whatever else the file says.
    pub usage: Option<String>,
    pub time: Option<Minutes>,
    /// Pellet, Plug or Leaf, or whatever else the file says.
    pub form: Option<String>,
}

/// A yeast pitched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YeastLine {
    pub name: String,
    pub laboratory: Option<String>,
    pub product_id: Option<String>,
    /// Liquid, Dry, Slant or Culture, or whatever else the file says.
    pub form: Option<String>,
}

/// A step of the mash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MashStepLine {
    pub name: String,
    /// Infusion, Temperature or Decoction, or whatever else the file says.
    pub kind: Option<String>,
    pub temperature: Option<Celsius>,
    pub time: Option<Minutes>,
}

/// A weight, in integer tenths of a gram: 30 g is 300.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Mass(i64);

impl Mass {
    #[must_use]
    pub const fn decigrams(self) -> i64 {
        self.0
    }
}

/// In g below 1 kg, with a decimal only when it has one (`30 g`, `2.5 g`); in kg from 1 kg, to
/// at most two decimals (`5.4 kg`, `5.25 kg`).
impl fmt::Display for Mass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 < 10_000 {
            write!(f, "{} g", tenths(self.0))
        } else {
            let hundredths = round_half_up(self.0, 100);
            let kg = format!("{}.{:02}", hundredths / 100, hundredths % 100);
            write!(f, "{} kg", kg.trim_end_matches('0').trim_end_matches('.'))
        }
    }
}

/// A volume, in integer tenths of a litre: 23.5 L is 235.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Litres(i64);

/// `23.5 L`, or `30 L` when whole.
impl fmt::Display for Litres {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} L", tenths(self.0))
    }
}

/// A temperature, in integer tenths of a degree Celsius: 66.5 °C is 665.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Celsius(i64);

/// `66.5 °C`, or `52 °C` when whole.
impl fmt::Display for Celsius {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} °C", tenths(self.0))
    }
}

/// A time in whole minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Minutes(i64);

/// `60 min`.
impl fmt::Display for Minutes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} min", self.0)
    }
}

/// Bitterness in whole International Bitterness Units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ibu(i64);

/// `32 IBU`.
impl fmt::Display for Ibu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} IBU", self.0)
    }
}

/// A colour in whole EBC, the European unit the Crew reads colour in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ebc(i64);

/// `14 EBC`.
impl fmt::Display for Ebc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} EBC", self.0)
    }
}

/// `value` tenths as a number with a decimal only when it has one: `23.5`, or `30`.
fn tenths(value: i64) -> String {
    if value % 10 == 0 {
        (value / 10).to_string()
    } else {
        format!("{}.{}", value / 10, value % 10)
    }
}

/// `1.044–1.060`.
impl fmt::Display for Range<Gravity> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}–{}", self.min, self.max)
    }
}

/// `30–45 IBU`.
impl fmt::Display for Range<Ibu> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}–{} IBU", self.min.0, self.max.0)
    }
}

/// `14–23 EBC`.
impl fmt::Display for Range<Ebc> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}–{} EBC", self.min.0, self.max.0)
    }
}

/// The largest value accepted from a file after scaling. Anything bigger is nonsense, and
/// keeps the integer maths far from overflowing.
const SCALED_MAX: f64 = 1e12;

/// `value × scale`, rounded to an integer, if `value` is a sane, non-negative number.
fn scaled(value: f64, scale: f64) -> Option<i64> {
    let scaled = (value * scale).round();
    if scaled.is_finite() && (0.0..=SCALED_MAX).contains(&scaled) {
        // In range and whole, so the cast is exact.
        #[allow(clippy::cast_possible_truncation)]
        Some(scaled as i64)
    } else {
        None
    }
}

fn gravity(sg: SpecificGravity) -> Option<i64> {
    scaled(sg.get(), 1000.0)
}

fn mass(kg: Kilograms) -> Option<Mass> {
    scaled(kg.get(), 10_000.0).map(Mass)
}

fn percent(percent: beerxml::units::Percent) -> Option<Percent> {
    scaled(percent.get(), 10.0).map(Percent::from_tenths)
}

fn minutes(minutes: beerxml::units::Minutes) -> Option<Minutes> {
    scaled(minutes.get(), 1.0).map(Minutes)
}

fn celsius(celsius: CrateCelsius) -> Option<Celsius> {
    scaled(celsius.get(), 10.0).map(Celsius)
}

fn ibu(ibu: beerxml::units::Ibu) -> Option<Ibu> {
    scaled(ibu.get(), 1.0).map(Ibu)
}

fn ebc(srm: beerxml::units::Srm) -> Option<Ebc> {
    scaled(srm.ebc(), 1.0).map(Ebc)
}

/// Text from the file, if it has any once trimmed.
fn text(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// The summary of the first Recipe in `bytes`.
fn summarise(bytes: &[u8]) -> Result<RecipeSummary, RecipeError> {
    let read = beerxml::read(bytes).map_err(|_| RecipeError::NotBeerXml)?;
    let recipe = read
        .document
        .recipes
        .into_iter()
        .next()
        .ok_or(RecipeError::NoRecipe)?;
    Ok(summary(recipe))
}

fn summary(recipe: beerxml::Recipe) -> RecipeSummary {
    RecipeSummary {
        name: recipe.name.trim().to_owned(),
        kind: text(recipe.kind.as_ref().map(beerxml::RecipeType::as_str)),
        style: recipe.style.as_ref().map(style),
        batch_size: recipe
            .batch_size
            .and_then(|litres| scaled(litres.get(), 10.0))
            .map(Litres),
        boil_time: recipe.boil_time.and_then(minutes),
        efficiency: recipe.efficiency.and_then(percent),
        targets: Targets {
            // EST_OG and EST_FG are the estimates a recipe aims for. Many exports, the
            // real-world sample among them, write the target into OG and FG instead.
            og: recipe
                .est_og
                .or(recipe.og)
                .and_then(gravity)
                .and_then(Gravity::og_target),
            fg: recipe
                .est_fg
                .or(recipe.fg)
                .and_then(gravity)
                .and_then(Gravity::fg_target),
            abv: recipe.est_abv.or(recipe.abv).and_then(percent),
            ibu: recipe.ibu.and_then(ibu),
            colour: recipe.est_color.and_then(ebc),
        },
        fermentables: fermentables(&recipe.fermentables),
        hops: recipe
            .hops
            .iter()
            .map(|hop| HopLine {
                name: hop.name.trim().to_owned(),
                amount: hop.amount.and_then(mass),
                alpha: hop.alpha.and_then(percent),
                usage: text(hop.usage.as_ref().map(beerxml::HopUse::as_str)),
                time: hop.time.and_then(minutes),
                form: text(hop.form.as_ref().map(beerxml::HopForm::as_str)),
            })
            .collect(),
        yeasts: recipe
            .yeasts
            .iter()
            .map(|yeast| YeastLine {
                name: yeast.name.trim().to_owned(),
                laboratory: text(yeast.laboratory.as_deref()),
                product_id: text(yeast.product_id.as_deref()),
                form: text(yeast.form.as_ref().map(beerxml::YeastForm::as_str)),
            })
            .collect(),
        mash_steps: recipe
            .mash
            .map(|mash| {
                mash.mash_steps
                    .iter()
                    .map(|step| MashStepLine {
                        name: step.name.trim().to_owned(),
                        kind: text(step.kind.as_ref().map(beerxml::MashStepType::as_str)),
                        temperature: step.step_temp.and_then(celsius),
                        time: step.step_time.and_then(minutes),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// The fermentables with each one's share of the grist: its weight over the total weight of
/// every fermentable that has one.
fn fermentables(fermentables: &[beerxml::Fermentable]) -> Vec<FermentableLine> {
    let amounts: Vec<Option<Mass>> = fermentables
        .iter()
        .map(|fermentable| fermentable.amount.and_then(mass))
        .collect();
    let total: i64 = amounts.iter().flatten().map(|amount| amount.0).sum();
    fermentables
        .iter()
        .zip(amounts)
        .map(|(fermentable, amount)| FermentableLine {
            name: fermentable.name.trim().to_owned(),
            amount,
            share: amount
                .filter(|_| total > 0)
                .map(|amount| Percent::from_tenths(round_half_up(amount.0 * 1000, total))),
        })
        .collect()
}

fn style(style: &beerxml::Style) -> StyleSummary {
    let og = |sg: Option<SpecificGravity>| sg.and_then(gravity).and_then(Gravity::og_target);
    let fg = |sg: Option<SpecificGravity>| sg.and_then(gravity).and_then(Gravity::fg_target);
    StyleSummary {
        name: style.name.trim().to_owned(),
        category: text(style.category.as_deref()),
        guide: text(style.style_guide.as_deref()),
        og: Range::of(og(style.og_min), og(style.og_max)),
        fg: Range::of(fg(style.fg_min), fg(style.fg_max)),
        ibu: Range::of(style.ibu_min.and_then(ibu), style.ibu_max.and_then(ibu)),
        colour: Range::of(style.color_min.and_then(ebc), style.color_max.and_then(ebc)),
    }
}

/// Recipes for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use super::{AttachedRecipe, Recipe, RecipeSummary};

    /// The crate's real-world sample: a Czech pale lager from Brewtlery, in UTF-8, with every
    /// section but targets for ABV, IBU and colour.
    pub const UNETICE: &[u8] = include_bytes!("fixtures/brewtlery_unetice_realworld_sample.xml");

    /// A small ISO-8859-1 Recipe whose Czech letters outside Latin-1 are character references.
    pub const LATIN1: &[u8] = include_bytes!("fixtures/latin1_unetice.xml");

    /// Two Recipes in one file.
    pub const TWO_RECIPES: &[u8] = include_bytes!("fixtures/two_recipes.xml");

    /// A hops library export: BeerXML, but no Recipe.
    pub const HOPS_ONLY: &[u8] = include_bytes!("fixtures/hops_only.xml");

    /// A Recipe with a name and nothing else.
    pub const SPARSE: &[u8] =
        b"<RECIPES><RECIPE><NAME>Just a name</NAME><VERSION>1</VERSION></RECIPE></RECIPES>";

    /// The summary of the real-world sample, with targets for ABV, IBU and colour added so the
    /// viewer shows every section.
    ///
    /// # Panics
    ///
    /// If the sample doesn't read, which is a bug in the fixtures.
    #[must_use]
    pub fn full() -> RecipeSummary {
        let mut summary = Recipe::parse(UNETICE.to_vec())
            .expect("the sample reads")
            .summary;
        summary.targets.abv = Some(super::Percent::from_tenths(44));
        summary.targets.ibu = Some(super::Ibu(36));
        summary.targets.colour = Some(super::Ebc(9));
        summary
    }

    /// The summary of a Recipe with just a name.
    ///
    /// # Panics
    ///
    /// If the fixture doesn't read, which is a bug in the fixtures.
    #[must_use]
    pub fn sparse() -> RecipeSummary {
        Recipe::parse(SPARSE.to_vec())
            .expect("the fixture reads")
            .summary
    }

    /// The full Recipe, attached.
    #[must_use]
    pub fn attached() -> AttachedRecipe {
        AttachedRecipe::Readable(Box::new(full()))
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{HOPS_ONLY, LATIN1, SPARSE, TWO_RECIPES, UNETICE};
    use super::*;

    fn summary(bytes: &[u8]) -> RecipeSummary {
        Recipe::parse(bytes.to_vec()).unwrap().summary
    }

    fn og(typed: &str) -> Gravity {
        Gravity::og(typed).unwrap()
    }

    fn fg(typed: &str) -> Gravity {
        Gravity::fg(typed).unwrap()
    }

    #[test]
    fn the_real_world_sample_maps_into_a_summary() {
        let recipe = summary(UNETICE);

        assert_eq!(recipe.name, "Únětický Vzor 10.7° (G70 30L)");
        assert_eq!(recipe.kind.as_deref(), Some("All Grain"));
        assert_eq!(recipe.batch_size.unwrap().to_string(), "30 L");
        assert_eq!(recipe.boil_time.unwrap().to_string(), "90 min");
        assert_eq!(recipe.efficiency.unwrap().to_string(), "75.0 %");
        assert_eq!(
            recipe.targets,
            Targets {
                og: Some(og("1.043")),
                fg: Some(fg("1.010")),
                abv: None,
                ibu: None,
                colour: None,
            }
        );

        let style = recipe.style.unwrap();
        assert_eq!(style.name, "Czech Premium Pale Lager");
        assert_eq!(style.category.as_deref(), Some("Pale Czech Lager"));
        assert_eq!(style.guide.as_deref(), Some("BJCP 2015"));
        assert_eq!(style.og.unwrap().to_string(), "1.044–1.060");
        assert_eq!(style.fg.unwrap().to_string(), "1.013–1.017");
        assert_eq!(style.ibu.unwrap().to_string(), "30–45 IBU");
        // 6.896 and 11.82 SRM.
        assert_eq!(style.colour.unwrap().to_string(), "14–23 EBC");

        assert_eq!(
            recipe.hops[3],
            HopLine {
                name: "Saaz".to_owned(),
                amount: Some(Mass(300)),
                alpha: Some(Percent::from_tenths(35)),
                usage: Some("Aroma".to_owned()),
                time: Some(Minutes(15)),
                form: Some("Pellet".to_owned()),
            }
        );
        assert_eq!(recipe.hops.len(), 4);
        assert_eq!(
            recipe.yeasts,
            [YeastLine {
                name: "NovaLager".to_owned(),
                laboratory: Some("Lallemand".to_owned()),
                product_id: Some("LalBrew NovaLager".to_owned()),
                form: Some("Dry".to_owned()),
            }]
        );
        let steps: Vec<String> = recipe
            .mash_steps
            .iter()
            .map(|step| {
                format!(
                    "{} {} {} {}",
                    step.name,
                    step.kind.as_deref().unwrap_or_default(),
                    step.temperature.unwrap(),
                    step.time.unwrap()
                )
            })
            .collect();
        assert_eq!(
            steps,
            [
                "Vystírka (Protein Rest) Infusion 52 °C 5 min",
                "Nižší cukrotvorná (Maltose) Temperature 63 °C 40 min",
                "Vyšší cukrotvorná (Dextrin) Temperature 72 °C 25 min",
                "Odrmutování (Mash Out) Temperature 78 °C 10 min",
            ]
        );
    }

    #[test]
    fn each_fermentable_has_its_share_of_the_grist() {
        let recipe = summary(UNETICE);

        let grist: Vec<(String, String, String)> = recipe
            .fermentables
            .iter()
            .map(|line| {
                (
                    line.name.clone(),
                    line.amount.unwrap().to_string(),
                    line.share.unwrap().to_string(),
                )
            })
            .collect();
        // 5.4 kg and 0.25 kg of 5.65 kg.
        assert_eq!(
            grist,
            [
                (
                    "Pilsner (Weyermann)".to_owned(),
                    "5.4 kg".to_owned(),
                    "95.6 %".to_owned()
                ),
                (
                    "Carapils (Weyermann)".to_owned(),
                    "250 g".to_owned(),
                    "4.4 %".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn a_fermentable_without_an_amount_has_no_share_and_takes_none_from_the_others() {
        let xml = b"<RECIPES><RECIPE><NAME>R</NAME><FERMENTABLES>
            <FERMENTABLE><NAME>Pils</NAME><AMOUNT>4</AMOUNT></FERMENTABLE>
            <FERMENTABLE><NAME>Mystery</NAME></FERMENTABLE>
            </FERMENTABLES></RECIPE></RECIPES>";

        let recipe = summary(xml);

        assert_eq!(recipe.fermentables[0].share.unwrap().to_string(), "100.0 %");
        assert_eq!(recipe.fermentables[1].amount, None);
        assert_eq!(recipe.fermentables[1].share, None);
    }

    #[test]
    fn a_file_of_several_recipes_gives_the_first() {
        let recipe = Recipe::parse(TWO_RECIPES.to_vec()).unwrap();

        assert_eq!(recipe.summary().name, "First Pale Ale");
        assert_eq!(recipe.summary().targets.og, Some(og("1.050")));
        assert_eq!(recipe.bytes(), TWO_RECIPES, "the whole file is kept");
    }

    #[test]
    fn an_iso_8859_1_file_reads_its_accented_names() {
        let recipe = summary(LATIN1);

        assert_eq!(recipe.name, "Únětický Vzor 10.7°");
        assert_eq!(recipe.fermentables[0].name, "Plzeňský slad");
        assert_eq!(recipe.batch_size.unwrap().to_string(), "20 L");
    }

    #[test]
    fn a_hops_only_file_has_no_recipe() {
        assert_eq!(
            Recipe::parse(HOPS_ONLY.to_vec()),
            Err(RecipeError::NoRecipe)
        );
    }

    #[test]
    fn a_file_that_isnt_beerxml_is_refused() {
        for bytes in [
            b"not xml at all".as_slice(),
            b"",
            b"<html><body>Hi</body></html>",
            b"\x89PNG\r\n\x1a\n",
        ] {
            assert_eq!(
                Recipe::parse(bytes.to_vec()),
                Err(RecipeError::NotBeerXml),
                "{bytes:?}"
            );
        }
    }

    #[test]
    fn a_file_over_the_cap_is_too_big() {
        let mut bytes = UNETICE.to_vec();
        bytes.resize(RECIPE_MAX_BYTES + 1, b' ');

        assert_eq!(Recipe::parse(bytes), Err(RecipeError::TooBig));
    }

    #[test]
    fn a_sparse_recipe_has_only_its_name() {
        let recipe = summary(SPARSE);

        assert_eq!(
            recipe,
            RecipeSummary {
                name: "Just a name".to_owned(),
                kind: None,
                style: None,
                batch_size: None,
                boil_time: None,
                efficiency: None,
                targets: Targets::default(),
                fermentables: Vec::new(),
                hops: Vec::new(),
                yeasts: Vec::new(),
                mash_steps: Vec::new(),
            }
        );
    }

    #[test]
    fn targets_prefer_the_estimates_and_drop_gravities_out_of_range() {
        let xml = b"<RECIPES><RECIPE><NAME>R</NAME>
            <OG>1.040</OG><EST_OG>1.048</EST_OG><FG>1.200</FG>
            <EST_ABV>4.75</EST_ABV><IBU>32.4</IBU><EST_COLOR>4</EST_COLOR>
            </RECIPE></RECIPES>";

        let targets = summary(xml).targets;

        assert_eq!(targets.og, Some(og("1.048")));
        assert_eq!(targets.fg, None, "an FG of 1.200 is out of range");
        assert_eq!(targets.abv.unwrap().to_string(), "4.8 %");
        assert_eq!(targets.ibu.unwrap().to_string(), "32 IBU");
        assert_eq!(targets.colour.unwrap().to_string(), "8 EBC");
    }

    #[test]
    fn masses_are_in_grams_below_a_kilo_and_kilos_from_one() {
        let cases = [
            (300, "30 g"),
            (25, "2.5 g"),
            (9_999, "999.9 g"),
            (10_000, "1 kg"),
            (54_000, "5.4 kg"),
            (52_500, "5.25 kg"),
            (52_549, "5.25 kg"),
            (52_550, "5.26 kg"),
        ];
        for (decigrams, shown) in cases {
            assert_eq!(Mass(decigrams).to_string(), shown, "{decigrams}");
        }
    }

    #[test]
    fn nonsense_numbers_are_dropped() {
        assert_eq!(scaled(-1.0, 10.0), None);
        assert_eq!(scaled(f64::NAN, 10.0), None);
        assert_eq!(scaled(f64::INFINITY, 10.0), None);
        assert_eq!(scaled(1e20, 10.0), None);
        // Exactly half rounds away from zero.
        assert_eq!(scaled(2.25, 10.0), Some(23));
    }
}
