//! Config-driven model pricing.
//!
//! Every price used by the tool comes from JSON pricing files. The embedded
//! defaults in `assets/pricing/*.json` are plain data files that can be copied,
//! edited and re-ordered by users through pricing profiles; this module no
//! longer hard-codes model names or price tables.
//!
//! A pricing file may scope a price to one or more data sources, so the same
//! model can be billed differently depending on where the usage came from.

use crate::model::{Source, Usage, UsageEvent};
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const TOKENS_PER_MILLION: f64 = 1_000_000.0;

/// Pricing layers shipped with the binary, lowest precedence first.
///
/// The names double as the file names written by `tkc pricing init`, so a
/// materialized profile directory behaves exactly like the embedded default.
pub const BUILTIN_PRICING_FILES: &[(&str, &str)] = &[
    (
        "00-defaults.json",
        include_str!("../assets/pricing/00-defaults.json"),
    ),
    (
        "10-openai.json",
        include_str!("../assets/pricing/10-openai.json"),
    ),
    (
        "20-anthropic.json",
        include_str!("../assets/pricing/20-anthropic.json"),
    ),
    (
        "30-google.json",
        include_str!("../assets/pricing/30-google.json"),
    ),
    (
        "40-deepseek.json",
        include_str!("../assets/pricing/40-deepseek.json"),
    ),
    (
        "50-mimo.json",
        include_str!("../assets/pricing/50-mimo.json"),
    ),
    (
        "60-moonshot.json",
        include_str!("../assets/pricing/60-moonshot.json"),
    ),
];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cost {
    pub usd: f64,
    pub unpriced_events: usize,
    pub unpriced_tokens: u64,
}

impl Cost {
    pub fn add_assign(&mut self, other: Cost) {
        self.usd += other.usd;
        self.unpriced_events += other.unpriced_events;
        self.unpriced_tokens += other.unpriced_tokens;
    }
}

/// How a data source reports cached input tokens.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AccountingMode {
    /// Cached tokens are already included in the plain input counter.
    #[serde(alias = "subset", alias = "input_includes_cached")]
    CachedInputSubset,
    /// Cached tokens are counted separately from the plain input counter.
    #[serde(alias = "separate", alias = "input_excludes_cached")]
    CachedInputSeparate,
}

impl AccountingMode {
    pub fn label(self) -> &'static str {
        match self {
            AccountingMode::CachedInputSubset => "cached_input_subset",
            AccountingMode::CachedInputSeparate => "cached_input_separate",
        }
    }
}

/// Accounting settings as written in a pricing file; every field is optional
/// so layers can be merged.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountingPatch {
    #[serde(default)]
    default: Option<AccountingMode>,
    #[serde(default)]
    by_source: BTreeMap<String, AccountingMode>,
    #[serde(default)]
    legacy_codex_event_subset: Option<bool>,
}

/// Accounting settings after merging every active pricing layer.
#[derive(Clone, Debug)]
pub struct Accounting {
    default_mode: AccountingMode,
    by_source: BTreeMap<Source, AccountingMode>,
    legacy_codex_event_subset: bool,
}

impl Default for Accounting {
    fn default() -> Self {
        Self {
            default_mode: AccountingMode::CachedInputSeparate,
            by_source: BTreeMap::new(),
            legacy_codex_event_subset: true,
        }
    }
}

impl Accounting {
    pub fn default_mode(&self) -> AccountingMode {
        self.default_mode
    }

    pub fn mode_for(&self, source: Source) -> AccountingMode {
        self.by_source
            .get(&source)
            .copied()
            .unwrap_or(self.default_mode)
    }

    pub fn legacy_codex_event_subset(&self) -> bool {
        self.legacy_codex_event_subset
    }

    pub fn source_overrides(&self) -> impl Iterator<Item = (Source, AccountingMode)> + '_ {
        self.by_source.iter().map(|(source, mode)| (*source, *mode))
    }

    fn mode_for_event(&self, event: &UsageEvent) -> AccountingMode {
        if self.legacy_codex_event_subset
            && event.source == Source::Codex
            && event.event_id == event.session_id
        {
            return AccountingMode::CachedInputSubset;
        }
        self.mode_for(event.source)
    }

    fn apply(&mut self, patch: &AccountingPatch) -> Result<()> {
        if let Some(default) = patch.default {
            self.default_mode = default;
        }
        for (source, mode) in &patch.by_source {
            let source = Source::parse(source).ok_or_else(|| {
                anyhow!("unknown source {source:?} in accounting.by_source; expected one of claude, codex, opencode, pi, kimi")
            })?;
            self.by_source.insert(source, *mode);
        }
        if let Some(legacy) = patch.legacy_codex_event_subset {
            self.legacy_codex_event_subset = legacy;
        }
        Ok(())
    }
}

/// USD prices per one million tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TokenPrices {
    pub input: f64,
    pub cached_input: f64,
    pub cache_creation_5m: f64,
    pub cache_creation_1h: f64,
    pub output: f64,
}

impl TokenPrices {
    fn cost(self, usage: &Usage, accounting: AccountingMode) -> f64 {
        let input_tokens = match accounting {
            AccountingMode::CachedInputSubset => usage.input.saturating_sub(usage.cached_input),
            AccountingMode::CachedInputSeparate => usage.input,
        };
        let output_tokens = usage.output + usage.reasoning_output;
        cost_component(input_tokens, self.input)
            + cost_component(usage.cached_input, self.cached_input)
            + cost_component(usage.cache_creation_input_5m, self.cache_creation_5m)
            + cost_component(usage.cache_creation_input_1h, self.cache_creation_1h)
            + cost_component(output_tokens, self.output)
    }
}

#[derive(Clone, Copy, Debug)]
struct ContextTier {
    prompt_tokens_over: u64,
    prices: TokenPrices,
}

/// A single price entry, optionally restricted to a set of data sources.
#[derive(Clone, Debug)]
struct PriceSpec {
    sources: Option<Vec<Source>>,
    unpriced: bool,
    note: Option<String>,
    base: TokenPrices,
    tiers: Vec<ContextTier>,
}

impl PriceSpec {
    fn prices_for(&self, usage: &Usage, accounting: AccountingMode) -> TokenPrices {
        if self.tiers.is_empty() {
            return self.base;
        }
        let prompt_tokens = prompt_tokens_for_tier(usage, accounting);
        let mut prices = self.base;
        for tier in &self.tiers {
            if prompt_tokens > tier.prompt_tokens_over {
                prices = tier.prices;
            }
        }
        prices
    }

    fn cost(&self, usage: &Usage, accounting: AccountingMode) -> f64 {
        self.prices_for(usage, accounting).cost(usage, accounting)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Matcher {
    /// Matches the model id exactly, or with a `-YYYY...` snapshot suffix.
    Plain,
    /// Matches any model id that starts with the key prefix (key ends with `*`).
    Wildcard,
}

#[derive(Clone, Debug)]
struct ModelRule {
    raw_key: String,
    normalized_key: String,
    matcher: Matcher,
    specs: Vec<PriceSpec>,
}

impl ModelRule {
    fn score(&self, model: &str) -> Option<usize> {
        match self.matcher {
            Matcher::Plain if model == self.normalized_key => {
                Some(2_000_000 + self.normalized_key.len())
            }
            Matcher::Plain => model
                .strip_prefix(&self.normalized_key)
                .is_some_and(is_snapshot_suffix)
                .then(|| 1_000_000 + self.normalized_key.len()),
            Matcher::Wildcard => {
                let prefix = wildcard_prefix(&self.normalized_key);
                model.starts_with(prefix).then_some(prefix.len())
            }
        }
    }

    fn spec_for(&self, source: Source) -> Option<(bool, &PriceSpec)> {
        if let Some(spec) = self
            .specs
            .iter()
            .find(|spec| spec.sources.as_ref().is_some_and(|s| s.contains(&source)))
        {
            return Some((true, spec));
        }
        self.specs
            .iter()
            .find(|spec| spec.sources.is_none())
            .map(|spec| (false, spec))
    }
}

/// One parsed pricing file, ready for lookups.
#[derive(Clone, Debug)]
pub struct PricingLayer {
    label: String,
    path: Option<PathBuf>,
    name: Option<String>,
    description: Option<String>,
    sources: Option<Vec<Source>>,
    accounting: Option<AccountingPatch>,
    rules: Vec<ModelRule>,
}

impl PricingLayer {
    pub fn from_path(path: &Path) -> Result<Self> {
        let content =
            fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        Self::from_json_str(&path.display().to_string(), &content)
            .with_context(|| format!("parse {}", path.display()))
            .map(|layer| layer.with_path(path))
    }

    pub fn from_json_str(label: &str, content: &str) -> Result<Self> {
        let file = serde_json::from_str::<PricingFile>(content)
            .map_err(|err| anyhow!("invalid pricing JSON in {label}: {err}"))?;
        let sources = file.sources()?;
        let accounting = file.accounting;
        if let Some(patch) = &accounting {
            // Fail fast on unknown sources instead of ignoring them at merge time.
            Accounting::default().apply(patch)?;
        }
        let mut rules = Vec::with_capacity(file.models.len());
        for (raw_key, value) in file.models {
            let specs = parse_model_entry(&raw_key, value)?;
            rules.push(compile_rule(&raw_key, specs, sources.as_deref())?);
        }
        Ok(Self {
            label: label.to_string(),
            path: None,
            name: file.name,
            description: file.description,
            sources,
            accounting,
            rules,
        })
    }

    pub fn embedded(name: &str, content: &str) -> Result<Self> {
        Self::from_json_str(&format!("builtin:{name}"), content)
    }

    fn with_path(mut self, path: &Path) -> Self {
        self.path = Some(path.to_path_buf());
        self
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn is_builtin(&self) -> bool {
        self.label.starts_with("builtin:")
    }

    pub fn model_count(&self) -> usize {
        self.rules.len()
    }

    pub fn accounting(&self) -> Option<&AccountingPatch> {
        self.accounting.as_ref()
    }

    /// Sources this whole file is restricted to, when declared at file level.
    pub fn sources(&self) -> Option<&[Source]> {
        self.sources.as_deref()
    }

    fn find(&self, source: Source, model: &str) -> Option<Found<'_>> {
        let mut candidates = self
            .rules
            .iter()
            .filter_map(|rule| rule.score(model).map(|score| (score, rule)))
            .collect::<Vec<_>>();
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.raw_key.cmp(&b.1.raw_key)));
        for (_, rule) in candidates {
            if let Some((source_scoped, spec)) = rule.spec_for(source) {
                return Some(Found {
                    layer: self,
                    key: &rule.raw_key,
                    source_scoped,
                    spec,
                });
            }
        }
        None
    }
}

#[derive(Clone, Copy, Debug)]
struct Found<'a> {
    layer: &'a PricingLayer,
    key: &'a str,
    source_scoped: bool,
    spec: &'a PriceSpec,
}

/// Result of resolving a model id against the active pricing layers.
#[derive(Clone, Copy, Debug)]
pub struct PriceMatch<'a> {
    pub layer: &'a str,
    pub path: Option<&'a Path>,
    pub key: &'a str,
    pub source_scoped: bool,
    pub unpriced: bool,
    pub prices: TokenPrices,
    pub note: Option<&'a str>,
}

/// Effective pricing: every active layer plus merged accounting settings.
#[derive(Clone, Debug)]
pub struct Pricing {
    /// Lowest precedence first; lookups walk this list in reverse.
    layers: Vec<PricingLayer>,
    accounting: Accounting,
}

impl Default for Pricing {
    fn default() -> Self {
        Self::empty()
    }
}

impl Pricing {
    pub fn empty() -> Self {
        Self {
            layers: Vec::new(),
            accounting: Accounting::default(),
        }
    }

    /// Embedded default pricing, materialized from `assets/pricing/*.json`.
    pub fn builtin() -> Result<Self> {
        let mut pricing = Self::empty();
        for (name, content) in BUILTIN_PRICING_FILES {
            pricing.push_layer(PricingLayer::embedded(name, content)?);
        }
        Ok(pricing)
    }

    /// Append a layer that outranks everything added so far.
    pub fn push_layer(&mut self, layer: PricingLayer) {
        self.layers.push(layer);
        self.accounting = Self::merge_accounting(&self.layers);
    }

    pub fn layers(&self) -> &[PricingLayer] {
        &self.layers
    }

    pub fn accounting(&self) -> &Accounting {
        &self.accounting
    }

    pub fn model_count(&self) -> usize {
        self.layers.iter().map(PricingLayer::model_count).sum()
    }

    pub fn event_cost(&self, event: &UsageEvent) -> Cost {
        let accounting = self.accounting.mode_for_event(event);
        let Some(found) = self.find(event.source, &event.model) else {
            return unpriced_cost(&event.usage);
        };
        if found.spec.unpriced {
            return unpriced_cost(&event.usage);
        }
        Cost {
            usd: found.spec.cost(&event.usage, accounting),
            unpriced_events: 0,
            unpriced_tokens: 0,
        }
    }

    pub fn lookup(&self, source: Source, model: &str) -> Option<PriceMatch<'_>> {
        let found = self.find(source, model)?;
        Some(PriceMatch {
            layer: found.layer.label(),
            path: found.layer.path(),
            key: found.key,
            source_scoped: found.source_scoped,
            unpriced: found.spec.unpriced,
            prices: found.spec.base,
            note: found.spec.note.as_deref(),
        })
    }

    pub fn unpriced_model_warnings<'a>(
        &self,
        events: impl Iterator<Item = &'a UsageEvent>,
    ) -> Vec<String> {
        self.unpriced_models(events)
            .into_iter()
            .map(|model| format!("No pricing configured for {model}; omitted from dollar totals"))
            .collect()
    }

    pub fn unpriced_models<'a>(&self, events: impl Iterator<Item = &'a UsageEvent>) -> Vec<String> {
        events
            .filter(|event| event.usage.computed_total() > 0)
            .filter(|event| {
                self.lookup(event.source, &event.model)
                    .is_none_or(|found| found.unpriced)
            })
            .map(|event| format!("{}/{}", event.source.label(), event.model))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn find(&self, source: Source, model: &str) -> Option<Found<'_>> {
        let normalized = normalized_model(model);
        if normalized.is_empty() || is_unpriceable_name(&normalized) {
            return None;
        }
        // Highest precedence first: the last layer pushed wins.
        self.layers
            .iter()
            .rev()
            .find_map(|layer| layer.find(source, &normalized))
    }

    fn merge_accounting(layers: &[PricingLayer]) -> Accounting {
        let mut accounting = Accounting::default();
        for layer in layers {
            if let Some(patch) = layer.accounting() {
                // Embedded layers are known-good; user files are validated when
                // the layer is compiled, so an error here cannot happen.
                let _ = accounting.apply(patch);
            }
        }
        accounting
    }
}

fn unpriced_cost(usage: &Usage) -> Cost {
    let total = usage.computed_total();
    if total == 0 {
        return Cost::default();
    }
    Cost {
        usd: 0.0,
        unpriced_events: 1,
        unpriced_tokens: total,
    }
}

/// A model entry is either one price object or a list of source-scoped ones.
fn parse_model_entry(raw_key: &str, value: serde_json::Value) -> Result<Vec<RawPriceSpec>> {
    let parse = |value: serde_json::Value| {
        serde_json::from_value::<RawPriceSpec>(value)
            .map_err(|err| anyhow!("pricing entry for {raw_key:?} is invalid: {err}"))
    };
    match value {
        serde_json::Value::Array(items) => items.into_iter().map(parse).collect(),
        other => parse(other).map(|spec| vec![spec]),
    }
}

fn compile_rule(
    raw_key: &str,
    raw_specs: Vec<RawPriceSpec>,
    file_sources: Option<&[Source]>,
) -> Result<ModelRule> {
    let normalized_key = normalized_model(raw_key);
    if normalized_key.is_empty() {
        return Err(anyhow!("pricing entry has an empty model name"));
    }
    if is_unpriceable_name(&normalized_key) {
        return Err(anyhow!(
            "invalid pricing model name {raw_key:?}; {normalized_key:?} is never billed"
        ));
    }
    let matcher = if normalized_key.ends_with('*') {
        if normalized_key.trim_end_matches('*').contains('*') {
            return Err(anyhow!(
                "pricing model {raw_key:?} may only use a single trailing wildcard"
            ));
        }
        if wildcard_prefix(&normalized_key).is_empty() {
            return Err(anyhow!(
                "pricing model {raw_key:?} needs a prefix before the wildcard"
            ));
        }
        Matcher::Wildcard
    } else {
        if normalized_key.contains('*') {
            return Err(anyhow!(
                "pricing model {raw_key:?} may only use a single trailing wildcard"
            ));
        }
        Matcher::Plain
    };

    if raw_specs.is_empty() {
        return Err(anyhow!("pricing model {raw_key:?} has no price entries"));
    }
    let mut specs = Vec::with_capacity(raw_specs.len());
    for raw_spec in raw_specs {
        specs.push(raw_spec.compile(raw_key, file_sources)?);
    }
    Ok(ModelRule {
        raw_key: raw_key.to_string(),
        normalized_key,
        matcher,
        specs,
    })
}

// ---------------------------------------------------------------------------
// Raw JSON shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct PricingFile {
    #[serde(default)]
    schema: Option<u32>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    sources: Option<Vec<String>>,
    #[serde(default)]
    accounting: Option<AccountingPatch>,
    #[serde(default)]
    models: BTreeMap<String, serde_json::Value>,
}

impl PricingFile {
    fn sources(&self) -> Result<Option<Vec<Source>>> {
        if let Some(schema) = self.schema {
            if schema != 1 && schema != 2 {
                return Err(anyhow!(
                    "unsupported pricing schema {schema}; supported schemas are 1 and 2"
                ));
            }
        }
        parse_sources(self.sources.as_deref(), "file level `sources`")
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPriceSpec {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    sources: Option<Vec<String>>,
    #[serde(default)]
    unpriced: bool,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    input: Option<f64>,
    #[serde(default)]
    cached_input: Option<f64>,
    #[serde(default, alias = "cache_creation_input_5m", alias = "cache_write_5m")]
    cache_creation_5m: Option<f64>,
    #[serde(default, alias = "cache_creation_input_1h", alias = "cache_write_1h")]
    cache_creation_1h: Option<f64>,
    #[serde(default)]
    output: Option<f64>,
    #[serde(default)]
    tiers: Vec<RawTier>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTier {
    #[serde(alias = "over_prompt_tokens", alias = "prompt_tokens_gt")]
    prompt_tokens_over: u64,
    #[serde(default)]
    input: Option<f64>,
    #[serde(default)]
    cached_input: Option<f64>,
    #[serde(default, alias = "cache_creation_input_5m", alias = "cache_write_5m")]
    cache_creation_5m: Option<f64>,
    #[serde(default, alias = "cache_creation_input_1h", alias = "cache_write_1h")]
    cache_creation_1h: Option<f64>,
    #[serde(default)]
    output: Option<f64>,
}

impl RawPriceSpec {
    fn sources(&self, model: &str) -> Result<Option<Vec<Source>>> {
        let mut declared = Vec::new();
        if let Some(source) = &self.source {
            let parsed = Source::parse(source).ok_or_else(|| {
                anyhow!("pricing for {model:?} has an unknown `source`: {source:?}")
            })?;
            declared.push(parsed);
        }
        if let Some(sources) = &self.sources {
            let parsed = parse_sources(Some(sources), &format!("pricing for {model:?}"))?;
            declared.extend(parsed.unwrap_or_default());
        }
        declared.sort();
        declared.dedup();
        Ok((!declared.is_empty()).then_some(declared))
    }

    fn compile(self, model: &str, file_sources: Option<&[Source]>) -> Result<PriceSpec> {
        let sources = self
            .sources(model)?
            .or_else(|| file_sources.map(<[Source]>::to_vec));
        if self.unpriced {
            if self.input.is_some() || self.output.is_some() {
                return Err(anyhow!(
                    "pricing for {model:?} sets `unpriced` together with token prices"
                ));
            }
            return Ok(PriceSpec {
                sources,
                unpriced: true,
                note: self.note,
                base: TokenPrices::default(),
                tiers: Vec::new(),
            });
        }

        let input = required_price(model, "input", self.input)?;
        let output = required_price(model, "output", self.output)?;
        let base = TokenPrices {
            input,
            cached_input: optional_price(model, "cached_input", self.cached_input, input)?,
            cache_creation_5m: optional_price(
                model,
                "cache_creation_5m",
                self.cache_creation_5m,
                0.0,
            )?,
            cache_creation_1h: optional_price(
                model,
                "cache_creation_1h",
                self.cache_creation_1h,
                0.0,
            )?,
            output,
        };

        let mut tiers = Vec::with_capacity(self.tiers.len());
        for tier in self.tiers {
            tiers.push(ContextTier {
                prompt_tokens_over: tier.prompt_tokens_over,
                prices: TokenPrices {
                    input: optional_price(model, "tiers.input", tier.input, base.input)?,
                    cached_input: optional_price(
                        model,
                        "tiers.cached_input",
                        tier.cached_input,
                        base.cached_input,
                    )?,
                    cache_creation_5m: optional_price(
                        model,
                        "tiers.cache_creation_5m",
                        tier.cache_creation_5m,
                        base.cache_creation_5m,
                    )?,
                    cache_creation_1h: optional_price(
                        model,
                        "tiers.cache_creation_1h",
                        tier.cache_creation_1h,
                        base.cache_creation_1h,
                    )?,
                    output: optional_price(model, "tiers.output", tier.output, base.output)?,
                },
            });
        }
        tiers.sort_by_key(|tier| tier.prompt_tokens_over);

        Ok(PriceSpec {
            sources,
            unpriced: false,
            note: self.note,
            base,
            tiers,
        })
    }
}

fn parse_sources(declared: Option<&[String]>, context: &str) -> Result<Option<Vec<Source>>> {
    let Some(declared) = declared else {
        return Ok(None);
    };
    let mut sources = Vec::with_capacity(declared.len());
    for value in declared {
        let source =
            Source::parse(value).ok_or_else(|| anyhow!("unknown source {value:?} in {context}"))?;
        sources.push(source);
    }
    sources.sort();
    sources.dedup();
    if sources.is_empty() {
        return Err(anyhow!("{context} must list at least one source"));
    }
    Ok(Some(sources))
}

fn required_price(model: &str, field: &str, value: Option<f64>) -> Result<f64> {
    let value = value.ok_or_else(|| anyhow!("pricing for {model:?} is missing {field}"))?;
    validate_price(model, field, value)
}

fn optional_price(model: &str, field: &str, value: Option<f64>, fallback: f64) -> Result<f64> {
    match value {
        Some(value) => validate_price(model, field, value),
        None => Ok(fallback),
    }
}

fn validate_price(model: &str, field: &str, value: f64) -> Result<f64> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(anyhow!(
            "pricing for {model:?} has invalid {field}: {value}"
        ))
    }
}

pub fn normalized_model(model: &str) -> String {
    model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-")
}

fn is_unpriceable_name(model: &str) -> bool {
    model == "unknown" || model == "<synthetic>"
}

fn wildcard_prefix(key: &str) -> &str {
    key.strip_suffix('*').unwrap_or(key)
}

fn is_snapshot_suffix(value: &str) -> bool {
    let Some(value) = value.strip_prefix('-') else {
        return false;
    };
    let mut chars = value.chars();
    matches!(
        (chars.next(), chars.next(), chars.next(), chars.next()),
        (Some('2'), Some('0'), Some(_), Some(_))
    )
}

fn prompt_tokens_for_tier(usage: &Usage, accounting: AccountingMode) -> u64 {
    match accounting {
        AccountingMode::CachedInputSubset => usage.input + usage.cache_creation_input,
        AccountingMode::CachedInputSeparate => {
            usage.input + usage.cached_input + usage.cache_creation_input
        }
    }
}

fn cost_component(tokens: u64, dollars_per_million: f64) -> f64 {
    tokens as f64 / TOKENS_PER_MILLION * dollars_per_million
}

#[cfg(test)]
mod tests {
    use super::{normalized_model, AccountingMode, Cost, Pricing, PricingLayer};
    use crate::model::{Source, Usage, UsageEvent};

    fn pricing(json: &str) -> Pricing {
        let mut pricing = Pricing::empty();
        pricing.push_layer(PricingLayer::from_json_str("test", json).unwrap());
        pricing
    }

    fn event(source: Source, model: &str, usage: Usage) -> UsageEvent {
        UsageEvent {
            source,
            event_id: String::from("event"),
            session_id: String::from("session"),
            date: String::from("2026-05-13"),
            project: String::from("/tmp/project"),
            model: model.to_string(),
            usage,
        }
    }

    fn legacy_codex_event(model: &str, usage: Usage) -> UsageEvent {
        let mut event = event(Source::Codex, model, usage);
        event.event_id = event.session_id.clone();
        event
    }

    fn builtin() -> Pricing {
        Pricing::builtin().unwrap()
    }

    #[test]
    fn embedded_defaults_cover_gpt_56_and_gpt_6_families() {
        let pricing = builtin();

        for (model, expected) in [
            ("gpt-5.6", 4.00),
            ("gpt-5.6-sol", 4.00),
            ("gpt-5.6-terra", 2.00),
            ("gpt-5.6-luna", 0.20),
            ("gpt-5.6-cyber", 12.50),
            ("gpt-6-astra", 10.00),
            ("gpt-6.1-sol", 2.00),
            ("gpt-6-sol", 2.00),
            ("gpt-6-luna", 0.10),
            ("gpt-6", 2.00),
            ("chat-latest", 5.00),
        ] {
            let found = pricing.lookup(Source::Codex, model).unwrap();
            assert_eq!(found.prices.input, expected, "{model}");
            assert!(!found.unpriced, "{model}");
            assert!(found.layer.starts_with("builtin:"), "{model}");
        }
    }

    #[test]
    fn prices_high_context_gpt_56_band_from_config_tiers() {
        let pricing = builtin();
        let low = pricing.event_cost(&event(
            Source::Codex,
            "gpt-5.6",
            Usage {
                input: 100_000,
                output: 1_000_000,
                total: 1_100_000,
                ..Usage::default()
            },
        ));
        let high = pricing.event_cost(&event(
            Source::Codex,
            "gpt-5.6",
            Usage {
                input: 300_000,
                output: 1_000_000,
                total: 1_300_000,
                ..Usage::default()
            },
        ));

        // Below 272k: 0.1M * 4.00 + 1M * 20.00.
        assert_eq!(format!("{:.2}", low.usd), "20.40");
        // Above 272k: input doubles, output x1.5.
        assert_eq!(format!("{:.2}", high.usd), "32.40");
    }

    #[test]
    fn distinguishes_dotted_and_hyphenated_claude_releases() {
        let pricing = builtin();
        let usage = Usage {
            input: 1_000_000,
            total: 1_000_000,
            ..Usage::default()
        };

        // opus 4.7 and opus 4-7 share the current price point.
        assert_eq!(
            pricing
                .event_cost(&event(Source::Claude, "claude-opus-4.7", usage.clone()))
                .usd,
            5.0
        );
        assert_eq!(
            pricing
                .event_cost(&event(Source::Claude, "claude-opus-4-7", usage.clone()))
                .usd,
            5.0
        );
        assert_eq!(
            pricing
                .event_cost(&event(Source::Claude, "claude-opus-5", usage.clone()))
                .usd,
            5.0
        );
        assert_eq!(
            pricing
                .event_cost(&event(Source::Claude, "claude-opus-5.5", usage.clone()))
                .usd,
            4.0
        );
        assert_eq!(
            pricing
                .event_cost(&event(Source::Claude, "claude-sonnet-5.5", usage.clone()))
                .usd,
            2.0
        );
        assert_eq!(
            pricing
                .event_cost(&event(Source::Claude, "claude-fable-5.1", usage.clone()))
                .usd,
            10.0
        );
        // Legacy opus 4.1 keeps the older, more expensive price point.
        assert_eq!(
            pricing
                .event_cost(&event(Source::Claude, "claude-opus-4-1-20250805", usage))
                .usd,
            15.0
        );
    }

    #[test]
    fn prices_claude_haiku_55_high_prompt_band() {
        let pricing = builtin();
        let low = pricing.event_cost(&event(
            Source::Claude,
            "claude-haiku-5.5",
            Usage {
                input: 100_000,
                output: 1_000_000,
                total: 1_100_000,
                ..Usage::default()
            },
        ));
        let high = pricing.event_cost(&event(
            Source::Claude,
            "claude-haiku-5.5",
            Usage {
                input: 150_000,
                output: 1_000_000,
                total: 1_150_000,
                ..Usage::default()
            },
        ));

        assert_eq!(format!("{:.2}", low.usd), "0.51");
        assert_eq!(format!("{:.2}", high.usd), "2.58");
    }

    #[test]
    fn prices_openai_cached_input_as_input_subset() {
        let cost = builtin().event_cost(&legacy_codex_event(
            "openai/gpt-5.4",
            Usage {
                input: 1_000_000,
                cached_input: 400_000,
                output: 200_000,
                total: 1_200_000,
                ..Usage::default()
            },
        ));

        assert_eq!(format!("{:.2}", cost.usd), "4.60");
        assert_eq!(cost.unpriced_events, 0);
    }

    #[test]
    fn prices_reasoning_output_as_output_tokens() {
        let cost = builtin().event_cost(&event(
            Source::Codex,
            "gpt-5.5",
            Usage {
                output: 100_000,
                reasoning_output: 50_000,
                total: 150_000,
                ..Usage::default()
            },
        ));

        assert_eq!(format!("{:.2}", cost.usd), "4.50");
        assert_eq!(cost.unpriced_events, 0);
    }

    #[test]
    fn prices_anthropic_cache_tokens_separately() {
        let cost = builtin().event_cost(&event(
            Source::Claude,
            "claude-sonnet-4.6",
            Usage {
                input: 1_000_000,
                cached_input: 2_000_000,
                cache_creation_input: 3_000_000,
                cache_creation_input_5m: 1_000_000,
                cache_creation_input_1h: 2_000_000,
                output: 100_000,
                total: 6_100_000,
                ..Usage::default()
            },
        ));

        assert_eq!(format!("{:.2}", cost.usd), "20.85");
        assert_eq!(cost.unpriced_events, 0);
    }

    #[test]
    fn marks_unknown_models_as_unpriced() {
        let cost = builtin().event_cost(&event(
            Source::Claude,
            "unknown-paid-model",
            Usage {
                total: 123,
                ..Usage::default()
            },
        ));

        assert_eq!(cost.usd, 0.0);
        assert_eq!(cost.unpriced_events, 1);
        assert_eq!(cost.unpriced_tokens, 123);
    }

    #[test]
    fn prices_openai_snapshots_and_claude_style_cache_shapes() {
        let pricing = builtin();
        let codex_cost = pricing.event_cost(&event(
            Source::Codex,
            "gpt-4.1-mini-2025-04-14",
            Usage {
                input: 1_000_000,
                cached_input: 100_000,
                output: 1_000_000,
                total: 2_000_000,
                ..Usage::default()
            },
        ));
        let claude_cost = pricing.event_cost(&event(
            Source::Claude,
            "openai/gpt-4.1-mini",
            Usage {
                input: 1_000_000,
                cached_input: 100_000,
                output: 1_000_000,
                total: 2_100_000,
                ..Usage::default()
            },
        ));

        assert_eq!(format!("{:.2}", codex_cost.usd), "2.01");
        assert_eq!(format!("{:.2}", claude_cost.usd), "2.01");
    }

    #[test]
    fn prices_common_claude_hyphenated_versions() {
        let cost = builtin().event_cost(&event(
            Source::Claude,
            "claude-3-5-haiku-20241022",
            Usage {
                input: 1_000_000,
                cached_input: 1_000_000,
                output: 1_000_000,
                total: 3_000_000,
                ..Usage::default()
            },
        ));

        assert_eq!(format!("{:.2}", cost.usd), "4.88");
        assert_eq!(cost.unpriced_events, 0);
    }

    #[test]
    fn prices_deepseek_and_mimo_families() {
        let pricing = builtin();
        let deepseek_cost = pricing.event_cost(&event(
            Source::Claude,
            "deepseek-v4-pro",
            Usage {
                input: 1_000_000,
                cached_input: 1_000_000,
                output: 1_000_000,
                total: 3_000_000,
                ..Usage::default()
            },
        ));
        let deepseek_flash_cost = pricing.event_cost(&event(
            Source::Claude,
            "deepseek-flash",
            Usage {
                input: 1_000_000,
                cached_input: 1_000_000,
                output: 1_000_000,
                total: 3_000_000,
                ..Usage::default()
            },
        ));
        let mimo_pro_cost = pricing.event_cost(&event(
            Source::Claude,
            "MiMo-V2.6-Pro",
            Usage {
                input: 100_000,
                cached_input: 100_000,
                output: 100_000,
                total: 300_000,
                ..Usage::default()
            },
        ));
        let mimo_flash_cost = pricing.event_cost(&event(
            Source::Claude,
            "MiMo-V2.6-Flash",
            Usage {
                input: 1_000_000,
                output: 1_000_000,
                total: 2_000_000,
                ..Usage::default()
            },
        ));

        assert_eq!(format!("{:.3}", deepseek_cost.usd), "5.324");
        assert_eq!(format!("{:.3}", deepseek_flash_cost.usd), "1.506");
        assert_eq!(format!("{:.5}", mimo_pro_cost.usd), "0.13086");
        assert_eq!(format!("{:.2}", mimo_flash_cost.usd), "0.42");
    }

    #[test]
    fn prices_gemini_kimi_and_moonshot_models() {
        let pricing = builtin();
        let gemini_cost = pricing.event_cost(&event(
            Source::Codex,
            "google/gemini-2.5-flash",
            Usage {
                input: 1_000_000,
                cached_input: 200_000,
                output: 1_000_000,
                total: 2_000_000,
                ..Usage::default()
            },
        ));
        let kimi_cost = pricing.event_cost(&event(
            Source::Claude,
            "moonshot/kimi-k2.6",
            Usage {
                input: 1_000_000,
                cached_input: 1_000_000,
                output: 1_000_000,
                total: 3_000_000,
                ..Usage::default()
            },
        ));
        let moonshot_cost = pricing.event_cost(&event(
            Source::Claude,
            "moonshot-v1-32k",
            Usage {
                input: 1_000_000,
                cached_input: 1_000_000,
                output: 1_000_000,
                total: 3_000_000,
                ..Usage::default()
            },
        ));

        assert_eq!(format!("{:.3}", gemini_cost.usd), "2.806");
        assert_eq!(format!("{:.2}", kimi_cost.usd), "5.11");
        assert_eq!(format!("{:.2}", moonshot_cost.usd), "5.00");
    }

    #[test]
    fn prices_gemini_high_context_tier_from_config() {
        let pricing = builtin();
        let low = pricing.event_cost(&event(
            Source::Codex,
            "gemini-3.1-pro-preview",
            Usage {
                input: 100_000,
                output: 1_000_000,
                total: 1_100_000,
                ..Usage::default()
            },
        ));
        let high = pricing.event_cost(&event(
            Source::Codex,
            "gemini-3.1-pro-preview",
            Usage {
                input: 300_000,
                output: 1_000_000,
                total: 1_300_000,
                ..Usage::default()
            },
        ));

        assert_eq!(format!("{:.2}", low.usd), "12.20");
        assert_eq!(format!("{:.2}", high.usd), "19.20");
    }

    #[test]
    fn only_warns_for_models_without_configured_prices() {
        let events = [
            event(
                Source::Claude,
                "deepseek-v4-pro",
                Usage {
                    total: 123,
                    ..Usage::default()
                },
            ),
            event(
                Source::Codex,
                "google/gemini-2.5-flash",
                Usage {
                    total: 456,
                    ..Usage::default()
                },
            ),
            event(
                Source::Claude,
                "unknown-paid-model",
                Usage {
                    total: 789,
                    ..Usage::default()
                },
            ),
        ];

        assert_eq!(
            builtin().unpriced_model_warnings(events.iter()),
            vec![
                "No pricing configured for claude/unknown-paid-model; omitted from dollar totals"
                    .to_string()
            ]
        );
    }

    #[test]
    fn legacy_flat_pricing_file_still_overrides_builtin_models() {
        let mut pricing = builtin();
        pricing.push_layer(
            PricingLayer::from_json_str(
                "custom",
                r#"{
                    "models": {
                        "openai/gpt-4.1-mini": {
                            "input": 10.0,
                            "cached_input": 1.0,
                            "output": 20.0
                        }
                    }
                }"#,
            )
            .unwrap(),
        );
        let cost = pricing.event_cost(&event(
            Source::Codex,
            "gpt-4.1-mini",
            Usage {
                input: 1_000_000,
                cached_input: 250_000,
                output: 1_000_000,
                total: 2_000_000,
                ..Usage::default()
            },
        ));

        assert_eq!(pricing.model_count(), builtin().model_count() + 1);
        assert_eq!(format!("{:.2}", cost.usd), "30.25");
        assert_eq!(cost.unpriced_events, 0);
    }

    #[test]
    fn custom_pricing_prices_unknown_models() {
        let pricing = pricing(
            r#"{
                "models": {
                    "my-proxy-model": {
                        "input": 2.0,
                        "cache_creation_5m": 3.0,
                        "cache_creation_1h": 4.0,
                        "cached_input": 0.5,
                        "output": 8.0
                    }
                }
            }"#,
        );
        let event = event(
            Source::Claude,
            "my-proxy-model",
            Usage {
                input: 1_000_000,
                cached_input: 1_000_000,
                cache_creation_input_5m: 1_000_000,
                cache_creation_input_1h: 1_000_000,
                output: 1_000_000,
                total: 5_000_000,
                ..Usage::default()
            },
        );

        assert_eq!(format!("{:.2}", pricing.event_cost(&event).usd), "17.50");
        assert!(pricing.unpriced_model_warnings([event].iter()).is_empty());
    }

    #[test]
    fn scoped_prices_let_one_model_cost_differently_per_source() {
        let mut pricing = builtin();
        pricing.push_layer(
            PricingLayer::from_json_str(
                "proxy",
                r#"{
                    "sources": ["claude"],
                    "models": {
                        "gpt-6-luna": { "input": 1.0, "cached_input": 0.1, "output": 2.0 }
                    }
                }"#,
            )
            .unwrap(),
        );
        let usage = Usage {
            input: 100_000,
            total: 100_000,
            ..Usage::default()
        };

        // The proxy file only claims the claude source, so codex keeps falling
        // back to the embedded OpenAI prices for the very same model id.
        let claude = pricing.event_cost(&event(Source::Claude, "gpt-6-luna", usage.clone()));
        let codex = pricing.event_cost(&event(Source::Codex, "gpt-6-luna", usage));

        assert_eq!(format!("{:.2}", claude.usd), "0.10");
        assert_eq!(format!("{:.2}", codex.usd), "0.01");
        let claude_match = pricing.lookup(Source::Claude, "gpt-6-luna").unwrap();
        let codex_match = pricing.lookup(Source::Codex, "gpt-6-luna").unwrap();
        assert_eq!(claude_match.layer, "proxy");
        assert!(claude_match.source_scoped);
        assert_eq!(codex_match.layer, "builtin:10-openai.json");
        assert!(!codex_match.source_scoped);
    }

    #[test]
    fn entry_level_source_scopes_are_matched_per_source() {
        let pricing = pricing(
            r#"{
                "models": {
                    "shared-model": [
                        { "source": "claude", "input": 10.0, "output": 20.0 },
                        { "source": "codex", "input": 1.0, "output": 2.0 }
                    ]
                }
            }"#,
        );
        let usage = Usage {
            input: 1_000_000,
            total: 1_000_000,
            ..Usage::default()
        };

        let claude = pricing.event_cost(&event(Source::Claude, "shared-model", usage.clone()));
        let codex = pricing.event_cost(&event(Source::Codex, "shared-model", usage.clone()));
        let opencode = pricing.event_cost(&event(Source::OpenCode, "shared-model", usage));

        assert_eq!(format!("{:.2}", claude.usd), "10.00");
        assert_eq!(format!("{:.2}", codex.usd), "1.00");
        assert_eq!(opencode.unpriced_events, 1);
        assert_eq!(opencode.usd, 0.0);
    }

    #[test]
    fn later_layers_win_over_earlier_layers() {
        let mut pricing = Pricing::empty();
        pricing.push_layer(
            PricingLayer::from_json_str(
                "first",
                r#"{ "models": { "layered-model": { "input": 1.0, "output": 1.0 } } }"#,
            )
            .unwrap(),
        );
        pricing.push_layer(
            PricingLayer::from_json_str(
                "second",
                r#"{ "models": { "layered-model": { "input": 9.0, "output": 9.0 } } }"#,
            )
            .unwrap(),
        );

        let cost = pricing.event_cost(&event(
            Source::Codex,
            "layered-model",
            Usage {
                input: 1_000_000,
                total: 1_000_000,
                ..Usage::default()
            },
        ));

        assert_eq!(format!("{:.2}", cost.usd), "9.00");
        assert_eq!(
            pricing
                .lookup(Source::Codex, "layered-model")
                .unwrap()
                .layer,
            "second"
        );
    }

    #[test]
    fn unpriced_entries_block_lower_layers() {
        let mut pricing = builtin();
        pricing.push_layer(
            PricingLayer::from_json_str(
                "blocker",
                r#"{ "models": { "gpt-5.6": { "unpriced": true } } }"#,
            )
            .unwrap(),
        );

        let cost = pricing.event_cost(&event(
            Source::Codex,
            "gpt-5.6",
            Usage {
                total: 1_000,
                ..Usage::default()
            },
        ));

        assert_eq!(cost.unpriced_events, 1);
        assert_eq!(cost.unpriced_tokens, 1_000);
        assert!(pricing.lookup(Source::Codex, "gpt-5.6").unwrap().unpriced);
    }

    #[test]
    fn wildcard_keys_match_model_families() {
        let pricing = pricing(r#"{ "models": { "proxy-*": { "input": 3.0, "output": 6.0 } } }"#);
        let cost = pricing.event_cost(&event(
            Source::Codex,
            "proxy-gpt-6-turbo",
            Usage {
                input: 1_000_000,
                total: 1_000_000,
                ..Usage::default()
            },
        ));

        assert_eq!(format!("{:.2}", cost.usd), "3.00");
    }

    #[test]
    fn global_accounting_modes_come_from_the_pricing_file() {
        let pricing = pricing(
            r#"{
                "accounting": {
                    "default": "cached_input_subset",
                    "by_source": { "codex": "cached_input_separate" },
                    "legacy_codex_event_subset": false
                },
                "models": { "shared-model": { "input": 1.0, "cached_input": 1.0, "output": 1.0 } }
            }"#,
        );
        let usage = Usage {
            input: 1_000_000,
            cached_input: 400_000,
            total: 1_000_000,
            ..Usage::default()
        };

        // Claude uses the file default: cached input is a subset of input.
        let claude = pricing.event_cost(&event(Source::Claude, "shared-model", usage.clone()));
        // Codex is overridden to separate accounting, and legacy detection is off.
        let codex = pricing.event_cost(&legacy_codex_event("shared-model", usage));

        assert_eq!(format!("{:.2}", claude.usd), "1.00");
        assert_eq!(format!("{:.2}", codex.usd), "1.40");
        assert_eq!(
            pricing.accounting().mode_for(Source::Claude),
            AccountingMode::CachedInputSubset
        );
        assert!(!pricing.accounting().legacy_codex_event_subset());
    }

    #[test]
    fn context_tiers_fall_back_to_base_prices() {
        let pricing = pricing(
            r#"{
                "models": {
                    "tiered-model": {
                        "input": 1.0,
                        "cached_input": 0.5,
                        "output": 2.0,
                        "tiers": [
                            { "prompt_tokens_over": 200000, "input": 4.0 }
                        ]
                    }
                }
            }"#,
        );
        let low = pricing.event_cost(&event(
            Source::Claude,
            "tiered-model",
            Usage {
                input: 100_000,
                output: 1_000_000,
                total: 1_100_000,
                ..Usage::default()
            },
        ));
        let high = pricing.event_cost(&event(
            Source::Claude,
            "tiered-model",
            Usage {
                input: 300_000,
                output: 1_000_000,
                total: 1_300_000,
                ..Usage::default()
            },
        ));

        assert_eq!(format!("{:.2}", low.usd), "2.10");
        assert_eq!(format!("{:.2}", high.usd), "3.20");
    }

    #[test]
    fn rejects_typos_in_price_entries() {
        let err = PricingLayer::from_json_str(
            "bad",
            r#"{ "models": { "m": { "imput": 1.0, "output": 1.0 } } }"#,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("unknown field"), "{err}");
    }

    #[test]
    fn rejects_invalid_prices_and_unknown_sources() {
        let bad_price = PricingLayer::from_json_str(
            "bad",
            r#"{ "models": { "bad-model": { "input": -1.0, "output": 1.0 } } }"#,
        )
        .unwrap_err();
        let bad_source = PricingLayer::from_json_str(
            "bad",
            r#"{ "models": { "m": { "sources": ["chrome"], "input": 1.0, "output": 1.0 } } }"#,
        )
        .unwrap_err();
        let missing_output =
            PricingLayer::from_json_str("bad", r#"{ "models": { "m": { "input": 1.0 } } }"#)
                .unwrap_err();
        let bad_schema =
            PricingLayer::from_json_str("bad", r#"{ "schema": 9, "models": {} }"#).unwrap_err();

        assert!(bad_price.to_string().contains("invalid input"));
        assert!(bad_source.to_string().contains("unknown source"));
        assert!(missing_output.to_string().contains("missing output"));
        assert!(bad_schema
            .to_string()
            .contains("unsupported pricing schema"));
    }

    #[test]
    fn normalizes_provider_prefixed_model_ids() {
        assert_eq!(normalized_model("openai/GPT-5.6_Pro"), "gpt-5.6-pro");
        assert_eq!(normalized_model("  gpt-6  "), "gpt-6");
    }

    #[test]
    fn empty_pricing_marks_every_billable_event_unpriced() {
        let cost = Pricing::empty().event_cost(&event(
            Source::Codex,
            "gpt-6",
            Usage {
                total: 10,
                ..Usage::default()
            },
        ));

        assert_eq!(
            cost,
            Cost {
                usd: 0.0,
                unpriced_events: 1,
                unpriced_tokens: 10
            }
        );
    }
}
