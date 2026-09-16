//! Reserved workspace work is an existing invocation executing the existing change transaction.
use crate::{service::Service, types::*, work_payload::Work};
use rack_ai_application::{
    ExecuteWorkUnit, ExecuteWorkUnitDependencies, RepositoryRegistry, WorkUnitRequest,
    WorkUnitWorkerSelection, WorkUnitWorkerSelector,
};
use rack_ai_infrastructure::{
    ConfiguredWorkspaceExecutor, FileSystemChangeManifestRepository, FileSystemRegistryRepository,
    FileSystemRepositoryRegistry, GitCommandWorktree, JCodeChangeImplementer, RegistryPaths,
    RegistryWorkUnitWorkerSelector, RepositoryPaths,
};
pub const MAX_WORKSPACE_SECONDS: u64 = 3600;
pub fn selection(
    service: &Service,
    input: (&Demand, &Work),
) -> Result<WorkUnitWorkerSelection, String> {
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
    let parsed = WorkUnitRequest::from_document(w.document(d)?)?;
    let paths = RegistryPaths::new(config.registry_root.clone());
    let models = FileSystemRegistryRepository::new(paths.clone()).load_models()?;
    let mut selections = Vec::new();
    for model in models
        .iter()
        .filter(|m| m.api_model_id.as_deref() == Some(d.profile.model.as_str()))
    {
        let selector = RegistryWorkUnitWorkerSelector::new(paths.clone())
            .for_reserved_worker(model.worker_id.clone());
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
    let implementer = JCodeChangeImplementer::new(paths.clone(), None).with_reserved_access(access);
    let selector = RegistryWorkUnitWorkerSelector::new(paths)
        .for_reserved_worker(selected.runtime().worker_id().into());
    let result = ExecuteWorkUnit::new(ExecuteWorkUnitDependencies {
        registry: &registry,
        command_policy: &policy,
        git: &GitCommandWorktree,
        manifests: &manifests,
        executor: Some(&executor),
        implementer: Some(&implementer),
        selector: &selector,
    })
    .execute(work.document(d)?)?;
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
