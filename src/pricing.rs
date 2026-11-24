//! Model prices: an embedded snapshot from community tables, with an
//! optional user override in the app's data directory.
//!
//! `assets/pricing.json` is the default snapshot. A file at
//! `SESSIONTREE_PRICING` or `<app data>/pricing.json` may add entries with the
//! same shape; its entries are tried first, so it can also override a rate.
use crate::paths::{app_dir, env_path};
use serde::Deserialize;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

const DEFAULT: &str = include_str!("../assets/pricing.json");

/// USD per million tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl Price {
    pub(crate) fn cost(self, input: u64, output: u64, cache_read: u64, cache_write: u64) -> f64 {
        (input as f64 * self.input
            + output as f64 * self.output
            + cache_read as f64 * self.cache_read
            + cache_write as f64 * self.cache_write)
            / 1_000_000.0
    }
}

#[derive(Deserialize, Default)]
struct Data {
    #[serde(default)]
    source: String,
    #[serde(default)]
    providers: Vec<Provider>,
    #[serde(default)]
    fallback: Vec<Entry>,
}

#[derive(Deserialize, Default)]
struct Provider {
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    entries: Vec<Entry>,
}

#[derive(Deserialize, Default)]
struct Entry {
    #[serde(default, rename = "match")]
    prefixes: Vec<String>,
    #[serde(default)]
    input: f64,
    #[serde(default)]
    output: f64,
    #[serde(default)]
    cache_read: f64,
    #[serde(default)]
    cache_write: f64,
}

impl Entry {
    fn matches(&self, model: &str) -> bool {
        self.prefixes
            .iter()
            .any(|prefix| model.starts_with(prefix.as_str()))
    }

    fn price(&self) -> Price {
        Price {
            input: self.input,
            output: self.output,
            cache_read: self.cache_read,
            cache_write: self.cache_write,
        }
    }
}

struct Pricing {
    providers: Vec<Provider>,
    fallback: Vec<Entry>,
    source: String,
    /// Changes when the snapshot or the user override changes, so the
    /// per-session cost cache can be discarded.
    fingerprint: u64,
}

impl Pricing {
    fn embedded() -> Self {
        Self::parse(DEFAULT).expect("assets/pricing.json is a valid price table")
    }

    fn parse(text: &str) -> Option<Self> {
        let data: Data = serde_json::from_str(text).ok()?;
        Some(Self {
            providers: data.providers,
            fallback: data.fallback,
            source: data.source,
            fingerprint: 0,
        })
    }

    fn load() -> Self {
        let mut pricing = Self::embedded();
        let mut hasher = DefaultHasher::new();
        DEFAULT.hash(&mut hasher);
        if let Some(path) = override_path()
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            text.hash(&mut hasher);
            match Self::parse(&text) {
                Some(user) => pricing.merge(user),
                None => log::warn!("pricing: {} is not a valid price table", path.display()),
            }
        }
        pricing.fingerprint = hasher.finish();
        pricing
    }

    /// User entries take precedence: they are tried before the snapshot.
    fn merge(&mut self, user: Pricing) {
        let mut providers = user.providers;
        providers.append(&mut self.providers);
        self.providers = providers;
        let mut fallback = user.fallback;
        fallback.append(&mut self.fallback);
        self.fallback = fallback;
        if !user.source.is_empty() {
            self.source = user.source;
        }
    }

    fn price_for(&self, provider: &str, model: &str) -> Option<Price> {
        let provider = provider.to_ascii_lowercase();
        let model = model.to_ascii_lowercase();
        if let Some(price) = self.provider_price(&provider, &model) {
            return Some(price);
        }
        if model.contains("free") {
            return None;
        }
        let bare = model.rsplit('/').next().unwrap_or(&model);
        if let Some(price) = self
            .fallback
            .iter()
            .find(|entry| entry.matches(bare))
            .map(Entry::price)
        {
            return Some(price);
        }
        infer_provider(bare).and_then(|provider| self.provider_price(provider, bare))
    }

    fn provider_price(&self, provider: &str, model: &str) -> Option<Price> {
        self.providers
            .iter()
            .filter(|group| {
                group
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(provider))
            })
            .find_map(|group| {
                group
                    .entries
                    .iter()
                    .find(|entry| entry.matches(model))
                    .map(Entry::price)
            })
    }
}

/// The provider an unknown-provider model name points at, as the old
/// fallback table did.
fn infer_provider(model: &str) -> Option<&'static str> {
    if model.starts_with("deepseek") {
        Some("deepseek")
    } else if model.starts_with("glm") {
        Some("zai")
    } else if model.starts_with("mimo") {
        Some("xiaomi")
    } else if model.starts_with("qwen") {
        Some("qwen")
    } else if model.starts_with("kimi") {
        Some("moonshot")
    } else {
        None
    }
}

fn override_path() -> Option<PathBuf> {
    env_path("SESSIONTREE_PRICING").or_else(|| Some(app_dir()?.join("pricing.json")))
}

static ACTIVE: RwLock<Option<Arc<Pricing>>> = RwLock::new(None);
static EMBEDDED: OnceLock<Pricing> = OnceLock::new();

/// Loads the snapshot and the user's override for the next scan.
pub(crate) fn reload() {
    *ACTIVE.write().unwrap_or_else(|error| error.into_inner()) = Some(Arc::new(Pricing::load()));
}

/// The price of `model` for `provider`, if the table has one.
pub(crate) fn price_for(provider: &str, model: &str) -> Option<Price> {
    {
        let active = ACTIVE.read().unwrap_or_else(|error| error.into_inner());
        if let Some(pricing) = active.as_ref() {
            return pricing.price_for(provider, model);
        }
    }
    EMBEDDED
        .get_or_init(Pricing::embedded)
        .price_for(provider, model)
}

/// Identifies the snapshot and the override loaded for this process.
pub(crate) fn fingerprint() -> u64 {
    let active = ACTIVE.read().unwrap_or_else(|error| error.into_inner());
    active.as_ref().map_or(0, |pricing| pricing.fingerprint)
}

/// Where the table came from, for the `--scan` report.
pub(crate) fn source() -> String {
    let active = ACTIVE.read().unwrap_or_else(|error| error.into_inner());
    active.as_ref().map_or_else(
        || EMBEDDED.get_or_init(Pricing::embedded).source.clone(),
        |pricing| pricing.source.clone(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed() -> Pricing {
        Pricing::parse(
            r#"{
                "providers": [
                    {"aliases": ["anthropic"], "entries": [
                        {"match": ["claude-sonnet"], "input": 3.0, "output": 15.0, "cache_read": 0.3, "cache_write": 3.75}
                    ]},
                    {"aliases": ["deepseek"], "entries": [
                        {"match": ["deepseek"], "input": 0.27, "output": 1.1, "cache_read": 0.07}
                    ]}
                ],
                "fallback": [
                    {"match": ["glm"], "input": 0.6, "output": 2.2, "cache_read": 0.11}
                ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn reads_the_embedded_snapshot() {
        let table = Pricing::embedded();
        assert!(table.price_for("anthropic", "claude-sonnet-4-5").is_some());
        assert!(table.price_for("openai", "gpt-5").is_some());
        assert!(
            table
                .price_for("deepseek-official", "deepseek-v4-flash")
                .is_some()
        );
    }

    #[test]
    fn provider_entries_win_and_free_models_stay_unpriced() {
        let table = parsed();
        let price = table.price_for("anthropic", "claude-sonnet-4").unwrap();
        assert_eq!(price.input, 3.0);
        assert_eq!(table.price_for("opencode", "claude-sonnet-4"), None);
        assert_eq!(
            table.price_for("opencode", "deepseek-v4.1-flash-free"),
            None
        );
    }

    #[test]
    fn unknown_providers_infer_the_model_family() {
        let table = parsed();
        assert!(table.price_for("opencode", "deepseek-chat").is_some());
        assert!(table.price_for("opencode", "glm-5.3").is_some());
        assert_eq!(table.price_for("opencode", "claude-sonnet-4"), None);
    }

    #[test]
    fn user_entries_come_before_the_snapshot() {
        let mut table = parsed();
        table.merge(
            Pricing::parse(
                r#"{"providers": [{"aliases": ["anthropic"], "entries": [
                    {"match": ["claude-sonnet"], "input": 9.0, "output": 9.0}
                ]}]}"#,
            )
            .unwrap(),
        );
        assert_eq!(
            table
                .price_for("anthropic", "claude-sonnet-4")
                .unwrap()
                .input,
            9.0
        );
    }
}
