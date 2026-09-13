use crate::{
    execution::ImageExecution, gpu::GpuProbe, lifecycle::Lifecycle, runtime::Runtime, types::*,
};
pub fn run(runtime: Runtime) -> Result<(), String> {
    loop {
        runtime.store.update(|state| {
            for job in &mut state.jobs {
                if !job.state.terminal() && !job.cleanup_pending {
                    if state.service.mode == Mode::Managed
                        && state.service.state == ServiceState::Starting
                    {
                        job.state = JobState::Starting;
                    }
                    if matches!(
                        state.service.state,
                        ServiceState::Waiting | ServiceState::RecoveryRequired
                    ) {
                        job.error = state.service.error.clone();
                    }
                }
                if !job.state.terminal() && !job.cleanup_pending && now() > job.deadline {
                    job.state = JobState::Failed;
                    job.error = Some("job admission deadline exceeded".into());
                    job.updated_at = now();
                }
            }
            Ok(())
        })?;
        {
            let _operation = crate::operation::lock(&runtime.store.root)?;
            let result = Lifecycle { runtime: &runtime }
                .tick()
                .and_then(|_| ImageExecution { runtime: &runtime }.tick());
            if let Err(error) = result {
                quarantine(&runtime, &error)?;
            }
        }
        runtime.store.update(|s| {
            s.service.heartbeat = now();
            Ok(())
        })?;
        std::thread::sleep(std::time::Duration::from_secs(
            crate::limits::HEARTBEAT_SECONDS,
        ));
    }
}
pub fn quarantine(runtime: &Runtime, error: &str) -> Result<(), String> {
    let mut service = runtime.store.read()?.service;
    service.mode = Mode::Closed;
    // Even when persistence fails, stale authority expires closed within 20 seconds.
    let closed = Lifecycle { runtime }.authority(&service);
    runtime.store.update(|s| {
        s.service.state = ServiceState::RecoveryRequired;
        s.service.error = Some(error.into());
        for job in &mut s.jobs {
            if job.cleanup_pending && !job.state.terminal() {
                job.state = JobState::Interrupted;
                job.error = Some(
                    "backend ownership or outcome requires recovery; no redispatch permitted"
                        .into(),
                );
                job.updated_at = now();
            }
        }
        Ok(())
    })?;
    closed
}
pub fn recover(runtime: &Runtime) -> Result<(), String> {
    let state = runtime.store.read()?;
    if matches!(
        state.service.state,
        ServiceState::Stopped | ServiceState::Waiting
    ) {
        if !runtime.systemd.gone()? {
            return quarantine(runtime, "unowned backend active at receiver startup");
        }
        return Ok(());
    }
    let mut closed = state.service.clone();
    closed.mode = Mode::Closed;
    Lifecycle { runtime }.authority(&closed)?;
    if state.service.state == ServiceState::Restarting {
        runtime
            .reservations
            .verify(state.service.lease.as_ref().ok_or("restart lost lease")?)?;
        if state.service.mode != Mode::Interactive || state.service.restart.is_none() {
            return quarantine(runtime, "invalid durable restart intent");
        }
        return Ok(());
    }
    if runtime.systemd.gone()?
        && (GpuProbe {
            config: &runtime.config,
        })
        .pids()?
        .is_empty()
    {
        if let Some(handle) = &state.service.lease {
            runtime.reservations.release(handle)?;
            return runtime.store.update(|s| {
                for j in &mut s.jobs {
                    if j.cleanup_pending {
                        j.cleanup_pending = false;
                        if !j.state.terminal() {
                            j.state = JobState::Interrupted;
                            j.error = Some("backend lost during receiver restart".into());
                        }
                    }
                }
                for session in &mut s.sessions {
                    if session.release_requested {
                        session.stopped = true;
                    }
                }
                s.service = Service::default();
                Ok(())
            });
        }
    }
    if state.service.state == ServiceState::Ready {
        if let Err(error) = (Lifecycle { runtime }).verify(&state.service) {
            return quarantine(runtime, &error);
        }
        // Resume only known prompt IDs on the same verified activation; dispatch intent is never reposted.
        return Ok(());
    }
    if matches!(
        state.service.state,
        ServiceState::Draining | ServiceState::Stopping | ServiceState::Starting
    ) {
        return Ok(());
    }
    quarantine(
        runtime,
        "interrupted/unknown ownership requires operator reconciliation",
    )
}
