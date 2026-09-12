use super::*;
pub(super) struct TaskSelection<'a> {
    pub services: RunNextTaskDependencies<'a>,
}
impl TaskSelection<'_> {
    pub(super) fn select_task(&self) -> Result<Selection, String> {
        let queued_tasks = self.services.execution_queue_repository.list()?;
        if queued_tasks.is_empty() {
            return Ok(Selection::NoneQueued);
        }
        for queued_task in queued_tasks {
            let task_id = TaskId::new(queued_task.task_id().to_string())?;
            let run_state = self.load_run_state(&task_id)?;
            let task_spec = self.services.task_spec_repository.load(&queued_task)?;
            let (run_state, active_node_id, placement) = self.plan_task(run_state, &task_spec)?;
            let blocked = self
                .services
                .lease_repository
                .blocked_resources(&placement)?;
            if !blocked.is_empty() {
                let updated = run_state.clone().with_metadata(
                    run_state
                        .metadata()
                        .clone()
                        .waiting_for_resources(queued_task.spec_path().to_string(), blocked),
                );
                self.services.run_state_repository.save(&updated)?;
                continue;
            }
            let ready = run_state.clone().with_metadata(
                run_state
                    .metadata()
                    .clone()
                    .ready(queued_task.spec_path().to_string()),
            );
            self.services.run_state_repository.save(&ready)?;
            let claimed = self
                .services
                .execution_queue_repository
                .claim(&queued_task)?;
            return Ok(Selection::Selected(SelectedTask {
                queued_task: claimed,
                run_state,
                task_spec,
                active_node_id,
                placement,
            }));
        }
        Ok(Selection::NoneAdmissible)
    }

    fn plan_task(
        &self,
        run_state: RunState,
        task_spec: &TaskSpec,
    ) -> Result<(RunState, Option<ActiveNodeId>, Placement), String> {
        if !task_spec.has_dag() {
            return Ok((run_state, None, task_spec.placement().clone()));
        }
        let run_state = self.ensure_dag_run_state(run_state, task_spec)?;
        let dag_run_state = run_state
            .dag_run_state()
            .cloned()
            .ok_or("dag run state missing".to_string())?;
        let node_id = task_spec
            .first_ready_node_id(&dag_run_state)
            .ok_or("no ready dag node available".to_string())?;
        let placement = task_spec.dag_node_placement(&node_id, self.services.worker_catalog)?;
        Ok((run_state, Some(node_id), placement))
    }

    fn ensure_dag_run_state(
        &self,
        run_state: RunState,
        task_spec: &TaskSpec,
    ) -> Result<RunState, String> {
        if run_state.dag_run_state().is_some() {
            return Ok(run_state);
        }
        let dag_run_state = task_spec
            .dag_run_state()?
            .ok_or("dag task was missing initial dag state".to_string())?;
        Ok(run_state.with_dag_run_state(dag_run_state))
    }

    fn load_run_state(&self, task_id: &TaskId) -> Result<RunState, String> {
        self.services
            .run_state_repository
            .find(task_id)?
            .ok_or("run state missing for queued task".to_string())
    }
}
