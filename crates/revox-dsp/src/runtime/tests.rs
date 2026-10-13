use super::*;

fn failure() -> ort::Error {
    ort::Error::new_with_code(ort::ErrorCode::EngineError, "injected failure")
}

#[test]
fn runtime_handles_allow_multiple_instances_and_reopening() {
    let first = OrtRuntime::new().unwrap();
    let second = OrtRuntime::new().unwrap();
    drop(first);
    drop(second);
    let reopened = OrtRuntime::new().unwrap();
    assert!(session_builder(&reopened.environment).is_ok());
}

#[test]
fn registers_all_providers_on_one_session_in_priority_order() {
    let chain = [Backend::TensorRt, Backend::Cuda, Backend::Cpu];
    let (session, report) = load_with_policy(
        &chain,
        || Ok(Vec::new()),
        |builder, backend| {
            builder.push(backend);
            Ok(())
        },
        |builder| Ok(builder.clone()),
    )
    .unwrap();
    assert_eq!(session, chain);
    assert_eq!(report.registered, chain);
    assert!(report.fallback.is_empty());
}

#[test]
fn failed_registration_does_not_skip_later_accelerators() {
    let (session, report) = load_with_policy(
        &[
            Backend::TensorRt,
            Backend::Cuda,
            Backend::DirectMl,
            Backend::Cpu,
        ],
        || Ok(Vec::new()),
        |builder, backend| {
            if backend == Backend::Cuda {
                return Err(failure());
            }
            builder.push(backend);
            Ok(())
        },
        |builder| Ok(builder.clone()),
    )
    .unwrap();
    assert_eq!(
        session,
        [Backend::TensorRt, Backend::DirectMl, Backend::Cpu]
    );
    assert_eq!(report.registered, session);
    assert_eq!(report.fallback.len(), 1);
    assert_eq!(report.fallback[0].attempted, [Backend::Cuda]);
    assert_eq!(report.fallback[0].stage, RuntimeStage::RegisterProvider);
}

#[test]
fn unavailable_accelerators_leave_a_working_cpu_session() {
    let (session, report) = load_with_policy(
        &[Backend::CoreMl, Backend::WebGpu, Backend::Cpu],
        || Ok(Vec::new()),
        |builder, backend| {
            if backend != Backend::Cpu {
                return Err(failure());
            }
            builder.push(backend);
            Ok(())
        },
        |builder| Ok(builder.clone()),
    )
    .unwrap();
    assert_eq!(session, [Backend::Cpu]);
    assert_eq!(report.registered, [Backend::Cpu]);
    assert_eq!(report.fallback.len(), 2);
    assert_eq!(report.fallback[0].attempted, [Backend::CoreMl]);
    assert_eq!(report.fallback[1].attempted, [Backend::WebGpu]);
    assert!(
        report
            .fallback
            .iter()
            .all(|failure| failure.stage == RuntimeStage::RegisterProvider)
    );
}

#[test]
fn cpu_only_chain_does_not_retry_a_load_failure() {
    let mut attempts = Vec::new();
    let result: Result<((), BackendReport), _> = load_with_policy(
        &[Backend::Cpu],
        || Ok(()),
        |_, backend| {
            attempts.push(backend);
            Ok(())
        },
        |_| Err(failure()),
    );
    assert_eq!(attempts, [Backend::Cpu]);
    assert!(matches!(
        result,
        Err(RuntimeError::Session {
            stage: RuntimeStage::LoadModel,
            ..
        })
    ));
}

#[test]
fn failed_chain_load_rebuilds_cpu_session_without_blame_on_one_provider() {
    let chain = [Backend::CoreMl, Backend::WebGpu, Backend::Cpu];
    let (session, report) = load_with_policy(
        &chain,
        || Ok(Vec::new()),
        |builder, backend| {
            builder.push(backend);
            Ok(())
        },
        |builder| {
            if builder.as_slice() == [Backend::Cpu] {
                Ok(builder.clone())
            } else {
                Err(failure())
            }
        },
    )
    .unwrap();
    assert_eq!(session, [Backend::Cpu]);
    assert_eq!(report.registered, [Backend::Cpu]);
    assert_eq!(report.fallback.len(), 1);
    assert_eq!(report.fallback[0].attempted, chain);
    assert_eq!(report.fallback[0].stage, RuntimeStage::LoadModel);
}

#[test]
fn failure_on_chain_and_cpu_retains_both_causes_and_registration_diagnostics() {
    let result: Result<((), BackendReport), _> = load_with_policy(
        &[Backend::NvRtx, Backend::Cuda, Backend::Cpu],
        || Ok(()),
        |_, backend| {
            if backend == Backend::NvRtx {
                Err(failure())
            } else {
                Ok(())
            }
        },
        |_| Err(failure()),
    );
    let RuntimeError::CpuFallback { accelerator, cpu } = result.unwrap_err() else {
        panic!("expected combined fallback error");
    };
    let RuntimeError::Session {
        backends, fallback, ..
    } = *accelerator
    else {
        panic!("expected provider chain error");
    };
    assert_eq!(backends, [Backend::Cuda, Backend::Cpu]);
    assert_eq!(fallback[0].attempted, [Backend::NvRtx]);
    let RuntimeError::Session { backends, .. } = *cpu else {
        panic!("expected CPU error");
    };
    assert_eq!(backends, [Backend::Cpu]);
}
