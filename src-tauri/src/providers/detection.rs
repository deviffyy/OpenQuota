use std::{collections::HashSet, sync::Arc};

use super::ProviderRegistry;

/// Probes local provider credentials without blocking Tauri's setup thread.
///
/// Each provider owns its credential sources and the probe remains local-only. The probes run on
/// separate blocking workers because some providers may consult the operating-system credential store.
pub async fn detect_local_credentials(
    registry: Arc<ProviderRegistry>,
    provider_ids: &[String],
) -> HashSet<String> {
    let mut probes = Vec::with_capacity(provider_ids.len());
    for provider_id in provider_ids {
        let Some(runtime) = registry.runtime(provider_id) else {
            continue;
        };
        let provider_id = provider_id.clone();
        probes.push(tauri::async_runtime::spawn_blocking(move || {
            (provider_id, runtime.has_local_credentials())
        }));
    }

    let mut detected = HashSet::new();
    for probe in probes {
        if let Ok((provider_id, true)) = probe.await {
            detected.insert(provider_id);
        }
    }
    detected
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        sync::{Arc, Barrier},
    };

    use crate::{
        models::{
            MetricDefinition, MetricSection, MetricSource, ProviderDefinition, ProviderSnapshot,
        },
        providers::{ProviderError, UsageProvider},
    };

    use super::{detect_local_credentials, ProviderRegistry};

    struct ProbeProvider {
        definition: ProviderDefinition,
        detected: bool,
        barrier: Arc<Barrier>,
    }

    impl UsageProvider for ProbeProvider {
        fn definition(&self) -> ProviderDefinition {
            self.definition.clone()
        }

        fn has_local_credentials(&self) -> bool {
            self.barrier.wait();
            self.detected
        }

        fn refresh(&self) -> Result<ProviderSnapshot, ProviderError> {
            unreachable!()
        }
    }

    fn provider(id: &str, detected: bool, barrier: Arc<Barrier>) -> Arc<dyn UsageProvider> {
        Arc::new(ProbeProvider {
            definition: ProviderDefinition {
                id: id.into(),
                display_name: id.into(),
                short_name: id.into(),
                fallback_enabled: true,
                local_usage_source_note: None,
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
            },
            detected,
            barrier,
        })
    }

    #[test]
    fn credential_probes_run_concurrently_and_return_only_hits() {
        let barrier = Arc::new(Barrier::new(2));
        let registry = Arc::new(
            ProviderRegistry::new(vec![
                provider("first", true, barrier.clone()),
                provider("second", false, barrier),
            ])
            .unwrap(),
        );
        let ids = vec!["first".to_owned(), "second".to_owned()];

        let detected = tauri::async_runtime::block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                detect_local_credentials(registry, &ids),
            )
            .await
            .expect("credential probes should overlap")
        });

        assert_eq!(detected, HashSet::from(["first".to_owned()]));
    }
}
