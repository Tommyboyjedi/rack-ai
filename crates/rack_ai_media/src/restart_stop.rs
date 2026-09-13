use crate::{
    backend_generation::BackendGeneration,
    gpu::GpuProbe,
    lifecycle::Lifecycle,
    restart_lifecycle::{InteractiveRestart, finishing},
    restart_state::*,
    runtime::Runtime,
    types::*,
};
pub struct RestartStop<'a> {
    pub runtime: &'a Runtime,
}
impl RestartStop<'_> {
    pub fn drain(&self, service: &Service) -> Result<(), String> {
        let r = self.runtime;
        Lifecycle { runtime: r }.verify(service)?;
        let gate = r.backend.barrier()?;
        if gate.activation != service.activation || gate.mode != Mode::Closed {
            return Err("restart gate barrier identity mismatch".into());
        }
        if r.backend.queue()?.empty() {
            InteractiveRestart { runtime: r }.phase(RestartPhase::Stopping)?;
            r.systemd.stop(service)?;
        } else if now()
            > service.restart.as_ref().ok_or("missing restart")?.since + r.config.drain_timeout
        {
            return Err("restart drain deadline; accepted work still active".into());
        }
        Ok(())
    }
    pub fn observe(&self, state: &MediaState) -> Result<(), String> {
        let r = self.runtime;
        let s = &state.service;
        let intent = s.restart.as_ref().ok_or("missing restart")?;
        let expected = if intent.phase == RestartPhase::Finishing {
            intent.target.as_ref().unwrap_or(&intent.previous)
        } else {
            &intent.previous
        };
        self.stopping_identity(expected)?;
        if r.systemd.gone()? && (GpuProbe { config: &r.config }).pids()?.is_empty() {
            if finishing(state, &r.config) || intent.phase == RestartPhase::Finishing {
                return crate::shutdown::Shutdown { runtime: r }.stopping(s);
            }
            return InteractiveRestart { runtime: r }.phase(RestartPhase::StartPending);
        }
        if now() > intent.since + r.config.stop_timeout {
            return Err("restart stop deadline; process ownership remains uncertain".into());
        }
        Ok(())
    }
    fn stopping_identity(&self, expected: &BackendGeneration) -> Result<(), String> {
        let observed = self.runtime.systemd.observe()?;
        if !observed.invocation.is_empty() && observed.invocation != expected.invocation
            || observed.pid != 0 && observed.pid != expected.pid
            || !observed.cgroup.is_empty() && observed.cgroup != expected.cgroup
        {
            return Err("unexpected replacement while stopping owned restart generation".into());
        }
        if observed.pid != 0
            && crate::backend_generation::present_start_ticks(observed.pid)?
                .is_some_and(|ticks| ticks != expected.start_ticks)
        {
            return Err("PID reused while stopping restart generation".into());
        }
        let gpu = GpuProbe {
            config: &self.runtime.config,
        };
        for pid in gpu
            .pids()?
            .into_iter()
            .chain((observed.pid != 0).then_some(observed.pid))
        {
            let groups = match std::fs::read_to_string(format!("/proc/{pid}/cgroup")) {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err("restart process ownership unavailable".into()),
            };
            if !groups
                .lines()
                .filter_map(|l| l.split_once("::"))
                .any(|(_, p)| std::path::Path::new(p).starts_with(&expected.cgroup))
            {
                return Err("foreign process during restart cleanup".into());
            }
        }
        Ok(())
    }
}
