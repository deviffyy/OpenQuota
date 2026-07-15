use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use crate::models::{MetricSection, MetricSource, ProviderCatalog, ProviderDefinition};

use super::UsageProvider;

const MAX_DEFAULT_PINS: usize = 2;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProviderRegistryError {
    #[error("Provider registry contains no providers.")]
    Empty,
    #[error("Provider definition is invalid: {0}")]
    Invalid(String),
}

pub struct ProviderRegistry {
    runtimes: HashMap<String, Arc<dyn UsageProvider>>,
    catalog: ProviderCatalog,
    definition_indices: HashMap<String, usize>,
    metric_indices: HashMap<String, (usize, usize)>,
}

impl ProviderRegistry {
    pub fn new(providers: Vec<Arc<dyn UsageProvider>>) -> Result<Self, ProviderRegistryError> {
        if providers.is_empty() {
            return Err(ProviderRegistryError::Empty);
        }

        let mut runtimes = HashMap::new();
        let mut definitions = Vec::with_capacity(providers.len());
        let mut definition_indices = HashMap::new();
        let mut metric_indices = HashMap::new();
        let mut metric_owners = BTreeMap::<String, String>::new();

        for provider in providers {
            let mut definition = provider.definition();
            definition.links = definition
                .links
                .iter()
                .filter_map(crate::models::ProviderLink::visible)
                .collect();
            if runtimes.contains_key(&definition.id) {
                return Err(invalid(format!(
                    "duplicate provider id `{}`",
                    definition.id
                )));
            }
            validate_definition(&definition, &metric_owners)?;
            let provider_index = definitions.len();
            definition_indices.insert(definition.id.clone(), provider_index);
            for (metric_index, metric) in definition.metrics.iter().enumerate() {
                metric_owners.insert(metric.id.clone(), definition.id.clone());
                metric_indices.insert(metric.id.clone(), (provider_index, metric_index));
            }
            runtimes.insert(definition.id.clone(), provider);
            definitions.push(definition);
        }
        if !definitions
            .iter()
            .any(|definition| definition.fallback_enabled)
        {
            return Err(invalid("registry has no fallback-enabled provider"));
        }

        Ok(Self {
            runtimes,
            catalog: ProviderCatalog {
                providers: definitions,
            },
            definition_indices,
            metric_indices,
        })
    }

    pub fn catalog(&self) -> &ProviderCatalog {
        &self.catalog
    }

    pub fn runtime(&self, id: &str) -> Option<Arc<dyn UsageProvider>> {
        self.runtimes.get(id).cloned()
    }

    pub fn definition(&self, id: &str) -> Option<&ProviderDefinition> {
        self.definition_indices
            .get(id)
            .and_then(|index| self.catalog.providers.get(*index))
    }

    pub fn metric(&self, id: &str) -> Option<&crate::models::MetricDefinition> {
        let (provider_index, metric_index) = *self.metric_indices.get(id)?;
        self.catalog
            .providers
            .get(provider_index)?
            .metrics
            .get(metric_index)
    }

    #[cfg(test)]
    pub fn from_definitions(
        definitions: Vec<ProviderDefinition>,
    ) -> Result<Self, ProviderRegistryError> {
        Self::new(
            definitions
                .into_iter()
                .map(|definition| {
                    Arc::new(DefinitionOnlyProvider(definition)) as Arc<dyn UsageProvider>
                })
                .collect(),
        )
    }
}

#[cfg(test)]
struct DefinitionOnlyProvider(ProviderDefinition);

#[cfg(test)]
impl UsageProvider for DefinitionOnlyProvider {
    fn definition(&self) -> ProviderDefinition {
        self.0.clone()
    }

    fn has_local_credentials(&self) -> bool {
        false
    }

    fn refresh(&self) -> Result<crate::models::ProviderSnapshot, super::ProviderError> {
        unreachable!()
    }
}

fn validate_definition(
    provider: &ProviderDefinition,
    metric_owners: &BTreeMap<String, String>,
) -> Result<(), ProviderRegistryError> {
    if provider.id.trim().is_empty() {
        return Err(invalid("provider id is empty"));
    }
    if provider.display_name.trim().is_empty() {
        return Err(invalid(format!(
            "provider `{}` has no display name",
            provider.id
        )));
    }
    if provider.short_name.trim().is_empty() {
        return Err(invalid(format!(
            "provider `{}` has no tray short name",
            provider.id
        )));
    }
    if provider.metrics.is_empty() {
        return Err(invalid(format!(
            "provider `{}` has no metrics",
            provider.id
        )));
    }

    let prefix = format!("{}.", provider.id);
    let mut local_ids = BTreeMap::<&str, ()>::new();
    let mut default_pins = 0;
    let mut has_visible_metric = false;

    for metric in &provider.metrics {
        if !metric.id.starts_with(&prefix) || metric.id.len() == prefix.len() {
            return Err(invalid(format!(
                "metric `{}` must use provider prefix `{prefix}`",
                metric.id
            )));
        }
        if local_ids.insert(metric.id.as_str(), ()).is_some()
            || metric_owners.contains_key(&metric.id)
        {
            return Err(invalid(format!("duplicate metric id `{}`", metric.id)));
        }
        if metric.label.trim().is_empty() {
            return Err(invalid(format!("metric `{}` has no label", metric.id)));
        }
        if metric
            .source
            .source_id()
            .is_some_and(|source| source.trim().is_empty())
        {
            return Err(invalid(format!(
                "metric `{}` has an empty source id",
                metric.id
            )));
        }
        if matches!(metric.source, MetricSource::Trend) && metric.pinnable {
            return Err(invalid(format!(
                "trend metric `{}` cannot be pinnable",
                metric.id
            )));
        }
        if metric.default_pinned && (!metric.pinnable || !metric.default_enabled) {
            return Err(invalid(format!(
                "metric `{}` has an invalid default pin",
                metric.id
            )));
        }
        if metric.pinnable != metric.tray.is_some() {
            return Err(invalid(format!(
                "metric `{}` has inconsistent tray metadata",
                metric.id
            )));
        }
        if metric
            .tray
            .as_ref()
            .is_some_and(|tray| tray.short_label.trim().is_empty())
        {
            return Err(invalid(format!(
                "metric `{}` has an empty tray label",
                metric.id
            )));
        }
        default_pins += usize::from(metric.default_pinned);
        has_visible_metric |=
            metric.default_enabled && metric.default_section == MetricSection::AlwaysVisible;
    }

    if default_pins > MAX_DEFAULT_PINS {
        return Err(invalid(format!(
            "provider `{}` has more than {MAX_DEFAULT_PINS} default pins",
            provider.id
        )));
    }
    if !has_visible_metric {
        return Err(invalid(format!(
            "provider `{}` has no default always-visible metric",
            provider.id
        )));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> ProviderRegistryError {
    ProviderRegistryError::Invalid(message.into())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        models::{
            MetricDefinition, MetricSection, MetricSource, ProviderDefinition, ProviderSnapshot,
        },
        providers::{ProviderError, UsageProvider},
    };

    use super::{ProviderRegistry, ProviderRegistryError};

    struct StubProvider(ProviderDefinition);

    impl UsageProvider for StubProvider {
        fn definition(&self) -> ProviderDefinition {
            self.0.clone()
        }

        fn has_local_credentials(&self) -> bool {
            false
        }

        fn refresh(&self) -> Result<ProviderSnapshot, ProviderError> {
            unreachable!()
        }
    }

    fn definition(id: &str) -> ProviderDefinition {
        ProviderDefinition {
            id: id.into(),
            display_name: "Provider".into(),
            short_name: "P".into(),
            fallback_enabled: true,
            local_usage_source_note: None,
            links: vec![],
            metrics: vec![MetricDefinition::new(
                format!("{id}.session"),
                "Session",
                MetricSource::Quota {
                    source_id: "session".into(),
                    session_window: true,
                },
                true,
                true,
                MetricSection::AlwaysVisible,
                true,
                Some("S"),
                None,
            )],
        }
    }

    fn runtime(definition: ProviderDefinition) -> Arc<dyn UsageProvider> {
        Arc::new(StubProvider(definition))
    }

    #[test]
    fn registry_preserves_definition_order_and_indexes_runtimes() {
        let registry = ProviderRegistry::new(vec![
            runtime(definition("first")),
            runtime(definition("second")),
        ])
        .unwrap();

        assert_eq!(
            registry
                .catalog()
                .providers
                .iter()
                .map(|provider| provider.id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert!(registry.runtime("second").is_some());
        assert!(registry.definition("first").is_some());
        assert!(registry.metric("first.session").is_some());
    }

    #[test]
    fn registry_exposes_only_trimmed_http_provider_links() {
        let mut provider = definition("links");
        provider.links = vec![
            crate::models::ProviderLink::new(" Status ", " https://status.example.com/ "),
            crate::models::ProviderLink::new("", "https://example.com/"),
            crate::models::ProviderLink::new("File", "file:///tmp/private"),
        ];

        let registry = ProviderRegistry::new(vec![runtime(provider)]).unwrap();

        assert_eq!(
            registry.definition("links").unwrap().links,
            vec![crate::models::ProviderLink::new(
                "Status",
                "https://status.example.com/"
            )]
        );
    }

    #[test]
    fn registry_rejects_duplicate_provider_and_metric_ids() {
        let duplicate_provider = ProviderRegistry::new(vec![
            runtime(definition("same")),
            runtime(definition("same")),
        ]);
        assert!(matches!(
            duplicate_provider,
            Err(ProviderRegistryError::Invalid(message)) if message.contains("duplicate provider")
        ));

        let mut duplicated = definition("metrics");
        duplicated.metrics.push(duplicated.metrics[0].clone());
        let duplicate_metric = ProviderRegistry::new(vec![runtime(duplicated)]);
        assert!(matches!(
            duplicate_metric,
            Err(ProviderRegistryError::Invalid(message)) if message.contains("duplicate metric")
        ));
    }

    #[test]
    fn registry_rejects_invalid_defaults_and_sources() {
        let mut invalid_pin = definition("pin");
        invalid_pin.metrics[0].pinnable = false;
        assert!(matches!(
            ProviderRegistry::new(vec![runtime(invalid_pin)]),
            Err(ProviderRegistryError::Invalid(message)) if message.contains("default pin")
        ));

        let mut hidden = definition("hidden");
        hidden.metrics[0].default_section = MetricSection::OnDemand;
        hidden.metrics[0].default_pinned = false;
        assert!(matches!(
            ProviderRegistry::new(vec![runtime(hidden)]),
            Err(ProviderRegistryError::Invalid(message)) if message.contains("always-visible")
        ));

        let mut empty_source = definition("source");
        empty_source.metrics[0].source = MetricSource::Quota {
            source_id: " ".into(),
            session_window: false,
        };
        assert!(matches!(
            ProviderRegistry::new(vec![runtime(empty_source)]),
            Err(ProviderRegistryError::Invalid(message)) if message.contains("empty source")
        ));

        let mut wrong_prefix = definition("prefix");
        wrong_prefix.metrics[0].id = "other.session".into();
        assert!(matches!(
            ProviderRegistry::new(vec![runtime(wrong_prefix)]),
            Err(ProviderRegistryError::Invalid(message)) if message.contains("provider prefix")
        ));

        let mut trend = definition("trend");
        trend.metrics[0].source = MetricSource::Trend;
        assert!(matches!(
            ProviderRegistry::new(vec![runtime(trend)]),
            Err(ProviderRegistryError::Invalid(message)) if message.contains("cannot be pinnable")
        ));

        let mut empty_tray = definition("tray");
        empty_tray.metrics[0].tray.as_mut().unwrap().short_label = " ".into();
        assert!(matches!(
            ProviderRegistry::new(vec![runtime(empty_tray)]),
            Err(ProviderRegistryError::Invalid(message)) if message.contains("empty tray label")
        ));

        let mut missing_tray = definition("missing-tray");
        missing_tray.metrics[0].tray = None;
        assert!(matches!(
            ProviderRegistry::new(vec![runtime(missing_tray)]),
            Err(ProviderRegistryError::Invalid(message)) if message.contains("inconsistent tray metadata")
        ));

        let mut no_fallback = definition("no-fallback");
        no_fallback.fallback_enabled = false;
        assert!(matches!(
            ProviderRegistry::new(vec![runtime(no_fallback)]),
            Err(ProviderRegistryError::Invalid(message)) if message.contains("no fallback-enabled")
        ));

        let mut too_many_pins = definition("pins");
        for suffix in ["weekly", "monthly"] {
            let mut metric = too_many_pins.metrics[0].clone();
            metric.id = format!("pins.{suffix}");
            if let MetricSource::Quota { source_id, .. } = &mut metric.source {
                *source_id = suffix.to_owned();
            }
            too_many_pins.metrics.push(metric);
        }
        assert!(matches!(
            ProviderRegistry::new(vec![runtime(too_many_pins)]),
            Err(ProviderRegistryError::Invalid(message)) if message.contains("more than 2 default pins")
        ));
    }

    #[test]
    fn builtin_provider_catalog_keeps_the_product_defaults() {
        use crate::providers::{antigravity, claude, codex, cursor};

        let registry = ProviderRegistry::new(vec![
            runtime(claude::definition()),
            runtime(codex::definition()),
            runtime(cursor::definition()),
            runtime(antigravity::definition()),
        ])
        .unwrap();
        let catalog = registry.catalog();

        assert_eq!(
            catalog
                .providers
                .iter()
                .map(|provider| provider.id.as_str())
                .collect::<Vec<_>>(),
            ["claude", "codex", "cursor", "antigravity"]
        );
        assert_eq!(
            registry
                .definition("codex")
                .unwrap()
                .metrics
                .iter()
                .map(|metric| metric.id.as_str())
                .collect::<Vec<_>>(),
            [
                "codex.session",
                "codex.weekly",
                "codex.spark",
                "codex.sparkWeekly",
                "codex.trend",
                "codex.credits",
                "codex.rateLimitResets",
                "codex.today",
                "codex.yesterday",
                "codex.last30",
            ]
        );
        assert!(registry.definition("codex").unwrap().fallback_enabled);
        assert!(registry.definition("claude").unwrap().fallback_enabled);
        assert!(registry.definition("cursor").unwrap().fallback_enabled);
        assert!(!registry.definition("antigravity").unwrap().fallback_enabled);
        assert!(registry
            .metric("claude.session")
            .unwrap()
            .source
            .session_window());

        let serialized = serde_json::to_value(catalog).unwrap();
        assert_eq!(serialized["providers"][1]["displayName"], "Codex");
        assert_eq!(
            serialized["providers"][1]["metrics"][6]["source"],
            serde_json::json!({"kind":"value","sourceId":"rateLimitResets"})
        );
        assert_eq!(
            serialized["providers"][1]["metrics"][9]["source"],
            serde_json::json!({"kind":"usage","period":"last30Days"})
        );
    }
}
