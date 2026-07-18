use std::{collections::HashSet, sync::Arc, time::Duration};

use super::ProviderRegistry;

const CREDENTIAL_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Probes local provider credentials without blocking Tauri's setup thread.
///
/// Each provider owns its credential sources and the probe remains local-only. The probes run on
/// separate blocking workers because some providers may consult the operating-system credential store.
pub async fn detect_local_credentials(
    registry: Arc<ProviderRegistry>,
    provider_ids: &[String],
) -> HashSet<String> {
    detect_local_credentials_with_timeout(registry, provider_ids, CREDENTIAL_PROBE_TIMEOUT).await
}

async fn detect_local_credentials_with_timeout(
    registry: Arc<ProviderRegistry>,
    provider_ids: &[String],
    timeout: Duration,
) -> HashSet<String> {
    let mut probes = Vec::with_capacity(provider_ids.len());
    for provider_id in provider_ids {
        let Some(runtime) = registry.runtime(provider_id) else {
            continue;
        };
        let provider_id = provider_id.clone();
        probes.push(tauri::async_runtime::spawn(async move {
            let worker =
                tauri::async_runtime::spawn_blocking(move || runtime.has_local_credentials());
            match tokio::time::timeout(timeout, worker).await {
                Ok(Ok(detected)) => Some((provider_id, detected)),
                Ok(Err(_)) => None,
                Err(_) => {
                    crate::app_warn!(
                        "providers",
                        "credential probe for {provider_id} reached its time limit"
                    );
                    None
                }
            }
        }));
    }

    let mut detected = HashSet::new();
    for probe in probes {
        if let Ok(Some((provider_id, true))) = probe.await {
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

    use super::{
        detect_local_credentials, detect_local_credentials_with_timeout, ProviderRegistry,
    };

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

    struct SlowProvider {
        definition: ProviderDefinition,
    }

    impl UsageProvider for SlowProvider {
        fn definition(&self) -> ProviderDefinition {
            self.definition.clone()
        }

        fn has_local_credentials(&self) -> bool {
            std::thread::sleep(std::time::Duration::from_millis(100));
            true
        }

        fn refresh(&self) -> Result<ProviderSnapshot, ProviderError> {
            unreachable!()
        }
    }

    #[test]
    fn stalled_credential_probe_does_not_hold_up_detection() {
        let definition = provider("slow", false, Arc::new(Barrier::new(1))).definition();
        let registry =
            Arc::new(ProviderRegistry::new(vec![Arc::new(SlowProvider { definition })]).unwrap());
        let ids = vec!["slow".to_owned()];

        let detected = tauri::async_runtime::block_on(detect_local_credentials_with_timeout(
            registry,
            &ids,
            std::time::Duration::from_millis(10),
        ));

        assert!(detected.is_empty());
    }
}
