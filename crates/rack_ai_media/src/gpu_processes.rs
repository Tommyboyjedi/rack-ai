use serde::Deserialize;
#[derive(Deserialize)]
struct Inventory {
    gpu: Vec<Adapter>,
}
#[derive(Deserialize)]
struct Adapter {
    uuid: String,
    processes: Processes,
}
#[derive(Deserialize)]
struct Processes {
    #[serde(default, rename = "process_info")]
    records: Vec<Process>,
    #[serde(default, rename = "$text")]
    availability: Option<String>,
}
#[derive(Deserialize)]
struct Process {
    pid: u32,
}
pub fn parse(xml: &str, expected_uuid: &str) -> Result<Vec<u32>, String> {
    let inventory: Inventory =
        quick_xml::de::from_str(xml).map_err(|_| "invalid GPU process inventory")?;
    if inventory.gpu.len() != 1 || inventory.gpu[0].uuid != expected_uuid {
        return Err("GPU process inventory UUID is ambiguous".into());
    }
    let processes = &inventory.gpu[0].processes;
    if processes
        .availability
        .as_ref()
        .is_some_and(|v| !v.trim().is_empty())
    {
        return Err("GPU process inventory is unavailable".into());
    }
    let mut pids = Vec::new();
    // Include every process_info entry: compute, graphics and mixed contexts.
    for process in &processes.records {
        if process.pid == 0 {
            return Err("invalid GPU process identity".into());
        }
        pids.push(process.pid);
    }
    pids.sort_unstable();
    pids.dedup();
    Ok(pids)
}
#[cfg(test)]
mod tests {
    use super::parse;
    #[test]
    fn includes_graphics_compute_and_mixed_processes() {
        let xml = r#"<nvidia_smi_log><gpu><uuid>owned</uuid><processes>
            <process_info><pid>10</pid><type>C</type></process_info>
            <process_info><pid>20</pid><type>G</type></process_info>
            <process_info><pid>30</pid><type>C+G</type></process_info>
            </processes></gpu></nvidia_smi_log>"#;
        assert_eq!(parse(xml, "owned").unwrap(), vec![10, 20, 30]);
    }
    #[test]
    fn absent_unsupported_and_wrong_uuid_fail_closed() {
        for xml in [
            "<nvidia_smi_log/>",
            "<nvidia_smi_log><gpu><uuid>owned</uuid></gpu></nvidia_smi_log>",
            "<nvidia_smi_log><gpu><uuid>owned</uuid><processes>N/A</processes></gpu></nvidia_smi_log>",
            "<nvidia_smi_log><gpu><uuid>other</uuid><processes/></gpu></nvidia_smi_log>",
            "<broken",
        ] {
            assert!(parse(xml, "owned").is_err());
        }
    }
    #[test]
    fn explicit_empty_inventory_is_free() {
        assert!(
            parse(
                "<nvidia_smi_log><gpu><uuid>owned</uuid><processes/></gpu></nvidia_smi_log>",
                "owned"
            )
            .unwrap()
            .is_empty()
        );
    }
}
