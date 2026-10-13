//! Ordered ONNX execution providers and explicit CPU recovery for DSP experiments.

use std::{
    path::Path,
    sync::{Arc, OnceLock},
};

use ort::{
    environment::Environment,
    ep,
    session::{
        Session,
        builder::{AutoDevicePolicy, GraphOptimizationLevel, SessionBuilder},
    },
};
use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Cpu,
    CoreMl,
    WebGpu,
    NvRtx,
    TensorRt,
    Cuda,
    DirectMl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStage {
    Configure,
    RegisterProvider,
    LoadModel,
}

/// A rejected registration or provider chain, retained after successful recovery.
#[derive(Clone, Debug, Serialize)]
pub struct BackendFallback {
    /// One provider for a registration failure; the entire successfully registered
    /// chain for a model-load failure, whose responsible provider is unknown.
    pub attempted: Vec<Backend>,
    pub stage: RuntimeStage,
    /// Diagnostic only; callers must not parse it to decide recovery behavior.
    pub diagnostic: String,
}

/// Configuration of the final session, not a measurement of operator placement.
///
/// All successfully registered providers share one session, in priority order.
/// Each operator may use a later provider or CPU. CoreML may select CPU, GPU, or
/// Neural Engine internally. Profiling and timing establish actual acceleration.
#[derive(Clone, Debug, Serialize)]
pub struct BackendReport {
    pub registered: Vec<Backend>,
    pub fallback: Vec<BackendFallback>,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("could not initialize ONNX Runtime")]
    Environment(#[source] Arc<ort::Error>),
    #[error("{backends:?} session failed during {stage:?}")]
    Session {
        backends: Vec<Backend>,
        stage: RuntimeStage,
        /// Earlier registration failures, if any, remain inspectable on failure.
        fallback: Vec<BackendFallback>,
        #[source]
        source: ort::Error,
    },
    #[error("provider chain and CPU fallback both failed")]
    CpuFallback {
        accelerator: Box<RuntimeError>,
        #[source]
        cpu: Box<RuntimeError>,
    },
}

// ort 2.0 rc.14 allows only one environment initialization per linked library,
// even after its last handle is dropped. Keep a strong handle so closing every
// plug-in instance does not prevent a later instance from creating sessions.
static ENVIRONMENT: OnceLock<Result<Environment, Arc<ort::Error>>> = OnceLock::new();

/// A handle to the shared ONNX environment used by Revox instances.
///
/// The environment lives for the library's lifetime. Model sessions and their
/// inference threads remain owned by each caller. No logging subscriber, signal
/// handler, or audio worker is installed here. Construction, inference, and
/// destruction belong off the audio callback.
pub struct OrtRuntime {
    environment: Environment,
}

impl OrtRuntime {
    /// # Errors
    /// Fails if the shared ONNX Runtime environment cannot initialize.
    pub fn new() -> Result<Self, RuntimeError> {
        let environment = ENVIRONMENT
            .get_or_init(|| {
                ort::init()
                    .with_name("heron-revox-dsp")
                    .with_telemetry(false)
                    .build()
                    .map_err(Arc::new)
            })
            .clone()
            .map_err(RuntimeError::Environment)?;
        Ok(Self { environment })
    }

    /// Registers the platform's bundled providers in order on one session.
    ///
    /// Failed accelerator registrations are recorded and later providers are
    /// still attempted. If the resulting accelerator chain cannot load the model,
    /// a fresh CPU-only session is tried once. Runtime inference failures remain
    /// errors; this constructor does not restart sessions during processing.
    ///
    /// # Errors
    /// Returns typed initialization errors, preserving registration diagnostics
    /// and both causes when model loading also fails on CPU.
    pub fn create_session(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<(Session, BackendReport), RuntimeError> {
        let path = path.as_ref();
        load_with_policy(
            platform_backends(),
            || session_builder(&self.environment),
            |builder, backend| provider(backend)?.register(builder),
            |builder| builder.commit_from_file(path),
        )
    }

    /// Loads embedded model bytes using the same provider chain and CPU recovery.
    ///
    /// The returned session owns the parsed model and an environment handle; it
    /// does not borrow `model` or this runtime handle.
    ///
    /// # Errors
    /// Returns typed initialization errors, preserving registration diagnostics
    /// and both causes when model loading also fails on CPU.
    pub fn create_session_from_memory(
        &self,
        model: &[u8],
    ) -> Result<(Session, BackendReport), RuntimeError> {
        load_with_policy(
            platform_backends(),
            || session_builder(&self.environment),
            |builder, backend| provider(backend)?.register(builder),
            |builder| builder.commit_from_memory(model),
        )
    }

    /// Specializes the plugin graph before provider partitioning and compilation.
    /// The same override is applied if model loading takes the CPU fallback path.
    pub(crate) fn create_session_from_memory_with_batch(
        &self,
        model: &[u8],
        channels: usize,
    ) -> Result<(Session, BackendReport), RuntimeError> {
        load_with_policy(
            platform_backends(),
            || {
                Ok(session_builder(&self.environment)?
                    .with_dimension_override("batch", channels as i64)?)
            },
            |builder, backend| provider(backend)?.register(builder),
            |builder| builder.commit_from_memory(model),
        )
    }
}

fn load_with_policy<T, B>(
    candidates: &[Backend],
    mut prepare: impl FnMut() -> ort::Result<B>,
    mut register: impl FnMut(&mut B, Backend) -> ort::Result<()>,
    mut load: impl FnMut(&mut B) -> ort::Result<T>,
) -> Result<(T, BackendReport), RuntimeError> {
    let initial = attempt_chain(candidates, &mut prepare, &mut register, &mut load);
    let accelerator = match initial {
        Ok(result) => return Ok(result),
        Err(error) => error,
    };
    let mut previous = match &accelerator {
        RuntimeError::Session {
            backends,
            stage: RuntimeStage::LoadModel,
            fallback,
            source,
        } if backends.iter().any(|backend| *backend != Backend::Cpu) => {
            let mut previous = fallback.clone();
            previous.push(BackendFallback {
                attempted: backends.clone(),
                stage: RuntimeStage::LoadModel,
                diagnostic: source.to_string(),
            });
            previous
        }
        _ => return Err(accelerator),
    };
    let (session, mut report) =
        attempt_chain(&[Backend::Cpu], &mut prepare, &mut register, &mut load).map_err(|cpu| {
            RuntimeError::CpuFallback {
                accelerator: Box::new(accelerator),
                cpu: Box::new(cpu),
            }
        })?;
    previous.append(&mut report.fallback);
    report.fallback = previous;
    Ok((session, report))
}

fn attempt_chain<T, B>(
    candidates: &[Backend],
    prepare: &mut impl FnMut() -> ort::Result<B>,
    register: &mut impl FnMut(&mut B, Backend) -> ort::Result<()>,
    load: &mut impl FnMut(&mut B) -> ort::Result<T>,
) -> Result<(T, BackendReport), RuntimeError> {
    let mut builder = prepare().map_err(|source| RuntimeError::Session {
        backends: candidates.to_vec(),
        stage: RuntimeStage::Configure,
        fallback: Vec::new(),
        source,
    })?;
    let mut report = BackendReport {
        registered: Vec::new(),
        fallback: Vec::new(),
    };
    for &backend in candidates {
        match register(&mut builder, backend) {
            Ok(()) => report.registered.push(backend),
            Err(source) if backend == Backend::Cpu => {
                return Err(RuntimeError::Session {
                    backends: vec![backend],
                    stage: RuntimeStage::RegisterProvider,
                    fallback: report.fallback,
                    source,
                });
            }
            Err(source) => report.fallback.push(BackendFallback {
                attempted: vec![backend],
                stage: RuntimeStage::RegisterProvider,
                diagnostic: source.to_string(),
            }),
        }
    }
    let session = load(&mut builder).map_err(|source| RuntimeError::Session {
        backends: report.registered.clone(),
        stage: RuntimeStage::LoadModel,
        fallback: report.fallback.clone(),
        source,
    })?;
    Ok((session, report))
}

fn session_builder(environment: &Environment) -> ort::Result<SessionBuilder> {
    Ok(Session::builder(environment)?
        .with_no_environment_execution_providers()?
        // Override ort's automatic NPU policy; our explicit chain owns selection.
        .with_auto_device(AutoDevicePolicy::PreferCPU)?
        .with_optimization_level(GraphOptimizationLevel::All)?
        .with_intra_threads(1)?
        .with_parallel_execution(false)?
        .with_intra_op_spinning(false)?
        .with_inter_op_spinning(false)?
        // Inputs vary at the final chunk; DirectML also requires this disabled.
        .with_memory_pattern(false)?)
}

fn platform_backends() -> &'static [Backend] {
    #[cfg(target_os = "macos")]
    return &[Backend::CoreMl, Backend::WebGpu, Backend::Cpu];
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return &[
        Backend::NvRtx,
        Backend::TensorRt,
        Backend::Cuda,
        Backend::DirectMl,
        Backend::Cpu,
    ];
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    return &[Backend::DirectMl, Backend::Cpu];
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    return &[
        Backend::NvRtx,
        Backend::TensorRt,
        Backend::Cuda,
        Backend::Cpu,
    ];
    #[cfg(not(any(
        target_os = "macos",
        all(
            any(target_os = "windows", target_os = "linux"),
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    )))]
    &[Backend::Cpu]
}

fn provider(backend: Backend) -> ort::Result<ep::ExecutionProviderDispatch> {
    match backend {
        Backend::Cpu => Ok(ep::CPU::default().with_arena_allocator(true).build()),
        #[cfg(target_os = "macos")]
        Backend::CoreMl => Ok(ep::CoreML::default()
            .with_compute_units(ep::coreml::ComputeUnits::All)
            .with_model_format(ep::coreml::ModelFormat::MLProgram)
            .build()),
        #[cfg(target_os = "macos")]
        Backend::WebGpu => Ok(ep::WebGPU::default().build()),
        #[cfg(all(
            target_os = "windows",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ))]
        Backend::DirectMl => Ok(ep::DirectML::default()
            .with_device_filter(ep::directml::DeviceFilter::Gpu)
            .with_performance_preference(ep::directml::PerformancePreference::HighPerformance)
            .build()),
        #[cfg(any(
            all(target_os = "windows", target_arch = "x86_64"),
            all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64")
            )
        ))]
        Backend::NvRtx => Ok(ep::NVRTX::default().build()),
        #[cfg(any(
            all(target_os = "windows", target_arch = "x86_64"),
            all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64")
            )
        ))]
        Backend::TensorRt => Ok(ep::TensorRT::default().build()),
        #[cfg(any(
            all(target_os = "windows", target_arch = "x86_64"),
            all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64")
            )
        ))]
        Backend::Cuda => Ok(ep::CUDA::default().build()),
        _ => Err(ort::Error::new(
            "execution provider is not compiled for this target",
        )),
    }
}

#[cfg(test)]
mod tests;
