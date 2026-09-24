use super::{compiler_output::SharedCompilerOutput, Error};
use crate::{
    compiler::DownloadCache, metrics, metrics::GuardedGauge, DetailedVersion, Fetcher, Language,
};
use anyhow::Context;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Semaphore;
use tracing::instrument;

#[async_trait]
pub trait EvmCompiler {
    type CompilerInput: CompilerInput;
    type CompilationError: CompilationError + for<'de> Deserialize<'de>;
    // TODO: parameterize version via: `type Version: Version`

    async fn compile(
        compiler_path: &Path,
        compiler_version: &DetailedVersion,
        input: &Self::CompilerInput,
    ) -> Result<Value, Error>;
}

pub trait CompilerInput: Serialize {
    fn normalize_output_selection(&mut self, version: &semver::Version);

    /// Modifies input so that the corresponding bytecode
    /// should have modified metadata hash, if any.
    fn modified_copy(&self) -> Self;

    fn language(&self) -> Language;

    fn settings(&self) -> Value;

    fn sources(&self) -> BTreeMap<String, String>;
}

pub trait CompilationError:
    foundry_compilers::CompilationError + for<'de> Deserialize<'de>
{
    fn formatted_message(&self) -> String;
}

#[derive(Clone, Debug)]
pub struct CompileResult<CompilerOutput> {
    pub output: CompilerOutput,
    pub raw: Value,
}

pub struct EvmCompilersPool<C: EvmCompiler> {
    cache: DownloadCache<DetailedVersion>,
    fetcher: Arc<dyn Fetcher<Version = DetailedVersion>>,
    threads_semaphore: Arc<Semaphore>,
    _phantom_data: PhantomData<C>,
}

impl<C: EvmCompiler> EvmCompilersPool<C> {
    pub fn new(
        fetcher: Arc<dyn Fetcher<Version = DetailedVersion>>,
        threads_semaphore: Arc<Semaphore>,
    ) -> Self {
        Self {
            cache: Default::default(),
            fetcher,
            threads_semaphore,
            _phantom_data: Default::default(),
        }
    }

    pub async fn load_from_dir(&self, dir: &PathBuf) {
        match self.cache.load_from_dir(dir).await {
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(
                    "cannot load local compilers from `{}` dir: {}",
                    dir.to_string_lossy(),
                    e
                )
            }
        }
    }

    pub fn normalize_compiler_version(
        &self,
        to_normalize: &DetailedVersion,
    ) -> Result<DetailedVersion, Error> {
        let is_matching_version = |version: &&DetailedVersion| {
            version.version() == to_normalize.version()
                && version.date() == to_normalize.date()
                && (version.commit().starts_with(to_normalize.commit())
                    || to_normalize.commit().starts_with(version.commit()))
        };

        let all_versions = self.fetcher.all_versions();
        let normalized = all_versions.iter().find(is_matching_version);

        if let Some(normalized) = normalized {
            Ok(normalized.clone())
        } else {
            Err(Error::CompilerNotFound(to_normalize.to_string()))
        }
    }

    #[instrument(name = "fetch_compiler", skip(self), level = "debug")]
    pub async fn fetch_compiler(&self, version: &DetailedVersion) -> Result<PathBuf, Error> {
        let path = self
            .cache
            .get(self.fetcher.as_ref(), version)
            .await
            .map_err(|err| {
                Error::Internal(anyhow::Error::new(err).context("fetching evm compiler"))
            })?;
        Ok(path)
    }

    #[instrument(name = "compile", skip(self, input), level = "debug")]
    pub async fn compile(
        &self,
        compiler_path: &Path,
        compiler_version: &DetailedVersion,
        input: &C::CompilerInput,
    ) -> Result<CompileResult<SharedCompilerOutput>, Error> {
        let raw = {
            let span = tracing::debug_span!(
                "compile contract with foundry-compilers",
                ver = compiler_version.to_string()
            );
            let _span_guard = span.enter();

            let _permit = {
                let _wait_timer_guard = metrics::COMPILATION_QUEUE_TIME.start_timer();
                let _wait_gauge_guard = metrics::COMPILATIONS_IN_QUEUE.guarded_inc();
                self.threads_semaphore
                    .acquire()
                    .await
                    .context("acquiring lock")?
            };

            let _compile_timer_guard = metrics::COMPILE_TIME.start_timer();
            let _compile_gauge_guard = metrics::COMPILATIONS_IN_FLIGHT.guarded_inc();

            C::compile(compiler_path, compiler_version, input).await?
        };

        validate_no_errors::<C::CompilationError>(&raw)?;
        let output: SharedCompilerOutput =
            serde_path_to_error::deserialize(&raw).context("deserializing compiler output")?;

        Ok(CompileResult { output, raw })
    }

    pub fn all_versions(&self) -> Vec<DetailedVersion> {
        self.fetcher.all_versions()
    }
}

#[derive(Debug, Clone, Deserialize)]
struct CompilerOutputErrors<E> {
    #[serde(default = "Vec::new")]
    pub errors: Vec<E>,
}

fn validate_no_errors<E: CompilationError>(raw_output: &Value) -> Result<(), Error> {
    let output_errors: CompilerOutputErrors<E> = serde_path_to_error::deserialize(raw_output)
        .context("deserializing compiler output errors")?;

    let mut errors = Vec::new();
    for error in output_errors.errors {
        if error.is_error() {
            errors.push(error.formatted_message());
        }
    }
    if !errors.is_empty() {
        return Err(Error::Compilation(errors));
    }

    Ok(())
}
