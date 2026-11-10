//! The engine builds a job and the runtime runs it end to end.
//!
//! The provider is a real script, written through the test-support helper so
//! the child writer owns every descriptor (the ETXTBSY case). No model is
//! called.

use std::time::Duration;

use er_agent::agent_slots::Workload;
use er_agent::decode::OutputProtocol;
use er_agent::job::{JobOutput, JobRunner, JobSpec, OutputContract};
use er_engine::agent_runtime::{
    resolve_invocation, AgentAccessProfile, AgentInvocationRequest, AgentSelection, AgentTaskKind,
};
use er_engine::config::{AiModelConfig, AiProviderConfig, ErConfig};
use er_engine::test_support::write_executable_script;

#[test]
fn a_job_runs_a_scripted_provider_and_decodes_its_text() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("provider.sh");
    write_executable_script(
        &script,
        "#!/bin/sh\nprintf '%s\\n' '{\"type\":\"result\",\"result\":\"hello from the job\"}'\n",
    )
    .unwrap();

    let mut config = ErConfig::default();
    config.ai_hub.providers.insert(
        "fake".into(),
        AiProviderConfig {
            command: script.to_string_lossy().into_owned(),
            // The family override injects the Claude stream-json flags; the
            // script ignores them and prints the event the decoder reads.
            family: Some("claude".into()),
            args: vec!["{prompt}".into()],
            models: vec![AiModelConfig {
                id: "fake-1".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    );

    let task = AgentTaskKind::CardReply;
    let invocation = resolve_invocation(
        &config,
        AgentInvocationRequest {
            selection: AgentSelection::Exact {
                provider_id: "fake",
                model_id: "fake-1",
            },
            task: &task,
            effort: None,
            effort_override: None,
            work_dir: dir.path().to_string_lossy().into_owned(),
            access: AgentAccessProfile::ReadOnly,
            live_logs: true,
        },
    )
    .expect("the fake provider resolves");

    let spec = JobSpec {
        kind: task.job_kind(),
        provider: invocation.provider_spec(Duration::from_secs(10)),
        prompt: "ignored".into(),
        work_dir: dir.path().to_string_lossy().into_owned(),
        workload: Workload::Background,
        output: OutputContract::Stdout,
    };
    assert_eq!(spec.provider.protocol, OutputProtocol::ClaudeStreamJson);

    let JobOutput { text, status, .. } = JobRunner::new(2, 2).run(&spec).expect("the job runs");
    assert_eq!(text, "hello from the job");
    assert!(status.success());
}
