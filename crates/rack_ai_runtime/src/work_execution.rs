//! Reserved workspace work is an existing invocation executing the existing change transaction.
use crate::{service::Service, types::*, work_payload::Work};
use rack_ai_application::{
    ExecuteWorkspace, ExecuteWorkspaceDependencies, RepositoryRegistry, WorkspaceWorkerSelection,
    WorkspaceWorkerSelector,
};
use rack_ai_infrastructure::{
    ConfiguredWorkspaceExecutor, FileSystemChangeManifestRepository, FileSystemRegistryRepository,
    FileSystemRepositoryRegistry, GitCommandWorktree, JCodeChangeImplementer, RegistryPaths,
    RegistryWorkspaceWorkerSelector, RepositoryPaths,
};
pub const MAX_WORKSPACE_SECONDS: u64 = 3600;
pub const WORKSPACE_RESPONSE_BYTES: u64 = 64 * 1024;
pub fn selection(
    service: &Service,
    input: (&Demand, &Work),
) -> Result<WorkspaceWorkerSelection, String> {
    let (d, w) = input;
    let config = service
        .config
        .workspace
        .as_ref()
        .ok_or("workspace_not_configured")?;
    let workspace = w.workspace().ok_or("workspace_required")?;
    if !d.profile.qualified
        || workspace.limits.max_implementation_attempts != 1
        || workspace.limits.timeout_seconds == 0
        || u64::from(workspace.limits.timeout_seconds) > MAX_WORKSPACE_SECONDS
    {
        return Err("workspace_limits_or_qualification".into());
    }
    let parsed = w.request(d)?;
    let context_window = reserved_context_window(d)?;
    let paths = RegistryPaths::new(config.registry_root.clone());
    let registry = FileSystemRepositoryRegistry::new(paths.clone());
    let policy = registry.command_policy()?;
    rack_ai_application::ChangeRequest::from_document(
        parsed.change.clone(),
        &rack_ai_application::ChangeRequestResolution {
            registry: &registry,
            command_policy: &policy,
            git: &GitCommandWorktree,
        },
    )?;
    let models = FileSystemRegistryRepository::new(paths.clone()).load_models()?;
    let mut selections = Vec::new();
    for model in models
        .iter()
        .filter(|m| m.api_model_id.as_deref() == Some(d.profile.model.as_str()))
    {
        let selector = RegistryWorkspaceWorkerSelector::new(paths.clone())
            .for_reserved_worker(model.worker_id.clone())
            .with_reserved_context_window(context_window);
        if let Ok(selected) = selector.select(&parsed)
            && selected
                .runtime()
                .worker_provenance()
                .is_some_and(|p| d.profile.resources.contains(&p.resource_id))
        {
            selections.push(selected);
        }
    }
    if selections.len() != 1 {
        return Err("reserved_worker_binding_unqualified_or_ambiguous".into());
    }
    selections.pop().ok_or("reserved_worker_missing".into())
}
fn reserved_context_window(d: &Demand) -> Result<u32, String> {
    if d.profile.max_input_tokens == 0 {
        return Err("reserved_context_window_missing".into());
    }
    Ok(d.profile.max_input_tokens)
}

pub fn execute(
    service: &Service,
    input: (&Demand, &Invocation),
) -> Result<serde_json::Value, String> {
    let (d, invocation) = input;
    let work = invocation.request.work.as_ref().ok_or("work_required")?;
    let selected = selection(service, (d, work))?;
    let config = service
        .config
        .workspace
        .as_ref()
        .ok_or("workspace_not_configured")?;
    let context_window = reserved_context_window(d)?;
    let paths = RegistryPaths::new(config.registry_root.clone());
    let registry = FileSystemRepositoryRegistry::new(paths.clone());
    let policy = registry.command_policy()?;
    let executor = ConfiguredWorkspaceExecutor::new(registry.executor_config()?)?;
    let manifests =
        FileSystemChangeManifestRepository::new(RepositoryPaths::new(config.state_root.clone()));
    let access = rack_ai_application::implement_worker_runtime::ReservedAccess {
        endpoint: format!(
            "http://{}/scoped/{}/{}/v1",
            service.config.listen, d.id, d.access_key
        ),
        invocation_id: invocation.id.clone(),
        authority_root: service.config.authority_root.clone(),
    };
    let implementer = JCodeChangeImplementer::new(paths.clone(), None)
        .with_reserved_context_window(context_window)
        .with_reserved_access(access);
    let selector = RegistryWorkspaceWorkerSelector::new(paths)
        .for_reserved_worker(selected.runtime().worker_id().into())
        .with_reserved_context_window(context_window);
    let result = ExecuteWorkspace::new(ExecuteWorkspaceDependencies {
        registry: &registry,
        command_policy: &policy,
        git: &GitCommandWorktree,
        manifests: &manifests,
        executor: Some(&executor),
        implementer: Some(&implementer),
        selector: &selector,
    })
    .execute(work.request(d)?)?;
    let value = serde_json::to_value(result).map_err(|e| e.to_string())?;
    if serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() as u64
        > invocation.response_bytes
    {
        return Err(format!(
            "workspace_result_exceeds_response_bound; evidence={}",
            value["packet_path"]
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::reserved_context_window;
    use crate::types::Demand;
    use serde_json::json;

    fn demand_with_max_input(max_input_tokens: u32) -> Demand {
        serde_json::from_value(json!({
            "id": "reserved-demand",
            "owner": "athba",
            "request": {
                "schema": crate::types::VERSION,
                "source_system": "athba",
                "work_id": "work",
                "acquisition_id": "acquire",
                "tag": "local-coder",
                "priority": "low",
                "capabilities": ["coding"],
                "context_tokens": 16368,
                "ttl_seconds": 60,
                "qualification": false
            },
            "priority": "low",
            "profile": {
                "tag": "local-coder",
                "version": "fixture",
                "model": "local-coder",
                "backend": "vllm",
                "driver": "fixture",
                "qualified": true,
                "evidence": ["fixture"],
                "capabilities": ["coding"],
                "context_tokens": 16368,
                "max_input_tokens": max_input_tokens,
                "max_output_tokens": 2048,
                "resources": ["gpu-2060"],
                "device_mib": {"gpu-2060": 16},
                "host_mib": 32,
                "cpu_percent": 100,
                "endpoint": "http://127.0.0.1:8018",
                "executable": "/usr/bin/true",
                "executable_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
                "args": [],
                "artifact": null,
                "artifact_sha256": null,
                "startup_seconds": 1,
                "drain_seconds": 1,
                "stop_seconds": 1,
                "inference_seconds": 1
            },
            "profile_hash": "hash",
            "state": "ready",
            "reason": null,
            "generation": "generation",
            "access_key": "access",
            "created": 1,
            "order": 1,
            "deadline": 9999999999_u64,
            "transition_deadline": 1,
            "victims": [],
            "process": null,
            "effect_started": false,
            "preflight_done": true,
            "released": false
        }))
        .unwrap()
    }

    #[test]
    fn reserved_context_window_uses_frozen_profile_max_input_tokens() {
        let mut demand = demand_with_max_input(14_320);
        assert_eq!(reserved_context_window(&demand).unwrap(), 14_320);

        demand.profile.max_input_tokens = 12_000;
        assert_eq!(reserved_context_window(&demand).unwrap(), 12_000);
    }

    #[test]
    fn reserved_context_window_missing_fails_closed() {
        let demand = demand_with_max_input(0);
        assert_eq!(
            reserved_context_window(&demand).unwrap_err(),
            "reserved_context_window_missing"
        );
    }
}
