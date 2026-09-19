use std::{
    collections::{HashMap, HashSet},
    fmt,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::camera::{CaptureBackend, DeviceEndpointKey};

pub const PROFILE_POLICY_SCHEMA_VERSION: u32 = 1;
pub const PROFILE_HASH_SCHEMA_VERSION: u32 = 1;
pub const MAX_FOURCC_VALUES: usize = 8;
pub const MAX_RESOLUTIONS: usize = 16;
pub const MAX_FPS_VALUES: usize = 16;
pub const MAX_CANDIDATES: usize = 256;
pub const MAX_DIMENSION: u32 = 8192;
pub const MAX_FPS: f64 = 1000.0;
pub const MAX_DURATION_MS: u64 = 600_000;
pub const MAX_METRIC_SAMPLES: usize = 100_000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModeCandidatePolicyV1 {
    pub schema_version: u32,
    pub fourcc: Vec<String>,
    pub resolutions: Vec<ResolutionV1>,
    pub fps: Vec<f64>,
    pub warmup_ms: u64,
    pub capture_only_ms: u64,
    pub first_frame_deadline_ms: u64,
    pub candidate_deadline_ms: u64,
    pub operation_deadline_ms: u64,
    pub shutdown_deadline_ms: u64,
    pub reopen_delay_ms: u64,
    pub minimum_fps_ratio: f64,
    pub maximum_read_failure_ratio: f64,
    pub maximum_gap_periods: f64,
    pub maximum_long_gap_ratio: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, Hash, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResolutionV1 {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FourCc([u8; 4]);

impl FourCc {
    pub fn parse(value: &str) -> Result<Self, ProfileConfigError> {
        let bytes = value.as_bytes();
        if bytes.len() != 4 || !bytes.iter().all(u8::is_ascii) {
            return Err(ProfileConfigError::InvalidFourCc);
        }
        let mut code = [0_u8; 4];
        code.copy_from_slice(bytes);
        Ok(Self(code))
    }

    pub fn as_str(&self) -> &str {
        // Construction proves all four bytes are ASCII and therefore UTF-8.
        std::str::from_utf8(&self.0).unwrap_or("")
    }
}

impl fmt::Display for FourCc {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModeCandidatePolicy {
    fourcc: Vec<FourCc>,
    resolutions: Vec<Resolution>,
    fps: Vec<f64>,
    warmup: Duration,
    capture_only: Duration,
    first_frame_deadline: Duration,
    candidate_deadline: Duration,
    operation_deadline: Duration,
    shutdown_deadline: Duration,
    reopen_delay: Duration,
    thresholds: CaptureThresholds,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureThresholds {
    pub minimum_fps_ratio: f64,
    pub maximum_read_failure_ratio: f64,
    pub maximum_gap_periods: f64,
    pub maximum_long_gap_ratio: f64,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProfileConfigError {
    #[error("unsupported profile policy schema version")]
    UnsupportedSchema,
    #[error("{field} must contain between 1 and {limit} values")]
    InvalidListSize { field: &'static str, limit: usize },
    #[error("FourCC must contain exactly four ASCII characters")]
    InvalidFourCc,
    #[error("candidate list contains a duplicate {field}")]
    DuplicateValue { field: &'static str },
    #[error("resolution dimensions must be within 1..={MAX_DIMENSION}")]
    InvalidResolution,
    #[error("FPS must be finite and within 0..={MAX_FPS}")]
    InvalidFps,
    #[error("{field} must be within 1..={MAX_DURATION_MS}")]
    InvalidDuration { field: &'static str },
    #[error("{field} is outside its supported range")]
    InvalidThreshold { field: &'static str },
    #[error("profile deadlines are inconsistent")]
    InconsistentDeadlines,
    #[error("candidate count exceeds the limit of {MAX_CANDIDATES}")]
    TooManyCandidates,
    #[error("profile policy could not be serialized canonically")]
    CanonicalSerialization,
}

impl ModeCandidatePolicy {
    pub fn validate(wire: ModeCandidatePolicyV1) -> Result<Self, ProfileConfigError> {
        if wire.schema_version != PROFILE_POLICY_SCHEMA_VERSION {
            return Err(ProfileConfigError::UnsupportedSchema);
        }
        validate_list_size("fourcc", wire.fourcc.len(), MAX_FOURCC_VALUES)?;
        validate_list_size("resolutions", wire.resolutions.len(), MAX_RESOLUTIONS)?;
        validate_list_size("fps", wire.fps.len(), MAX_FPS_VALUES)?;

        let fourcc = wire
            .fourcc
            .iter()
            .map(|value| FourCc::parse(value))
            .collect::<Result<Vec<_>, _>>()?;
        require_unique("FourCC", fourcc.iter().copied())?;

        let resolutions = wire
            .resolutions
            .iter()
            .map(|value| {
                if value.width == 0
                    || value.height == 0
                    || value.width > MAX_DIMENSION
                    || value.height > MAX_DIMENSION
                {
                    Err(ProfileConfigError::InvalidResolution)
                } else {
                    Ok(Resolution {
                        width: value.width,
                        height: value.height,
                    })
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        require_unique("resolution", resolutions.iter().copied())?;

        if wire
            .fps
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0 || *value > MAX_FPS)
        {
            return Err(ProfileConfigError::InvalidFps);
        }
        let mut fps_bits = HashSet::with_capacity(wire.fps.len());
        if wire
            .fps
            .iter()
            .any(|value| !fps_bits.insert(value.to_bits()))
        {
            return Err(ProfileConfigError::DuplicateValue { field: "FPS" });
        }

        let candidate_count = fourcc
            .len()
            .checked_mul(resolutions.len())
            .and_then(|count| count.checked_mul(wire.fps.len()))
            .ok_or(ProfileConfigError::TooManyCandidates)?;
        if candidate_count > MAX_CANDIDATES {
            return Err(ProfileConfigError::TooManyCandidates);
        }

        for (field, value) in [
            ("warmup_ms", wire.warmup_ms),
            ("capture_only_ms", wire.capture_only_ms),
            ("first_frame_deadline_ms", wire.first_frame_deadline_ms),
            ("candidate_deadline_ms", wire.candidate_deadline_ms),
            ("operation_deadline_ms", wire.operation_deadline_ms),
            ("shutdown_deadline_ms", wire.shutdown_deadline_ms),
            ("reopen_delay_ms", wire.reopen_delay_ms),
        ] {
            if value == 0 || value > MAX_DURATION_MS {
                return Err(ProfileConfigError::InvalidDuration { field });
            }
        }
        validate_ratio("minimum_fps_ratio", wire.minimum_fps_ratio, false)?;
        validate_ratio(
            "maximum_read_failure_ratio",
            wire.maximum_read_failure_ratio,
            true,
        )?;
        validate_ratio("maximum_long_gap_ratio", wire.maximum_long_gap_ratio, true)?;
        if !wire.maximum_gap_periods.is_finite() || wire.maximum_gap_periods < 1.0 {
            return Err(ProfileConfigError::InvalidThreshold {
                field: "maximum_gap_periods",
            });
        }

        let minimum_candidate_ms = wire
            .first_frame_deadline_ms
            .checked_add(wire.warmup_ms)
            .and_then(|value| value.checked_add(wire.capture_only_ms))
            .ok_or(ProfileConfigError::InconsistentDeadlines)?;
        if wire.candidate_deadline_ms < minimum_candidate_ms
            || wire.operation_deadline_ms < wire.candidate_deadline_ms
            || wire.shutdown_deadline_ms > wire.candidate_deadline_ms
        {
            return Err(ProfileConfigError::InconsistentDeadlines);
        }

        Ok(Self {
            fourcc,
            resolutions,
            fps: wire.fps,
            warmup: Duration::from_millis(wire.warmup_ms),
            capture_only: Duration::from_millis(wire.capture_only_ms),
            first_frame_deadline: Duration::from_millis(wire.first_frame_deadline_ms),
            candidate_deadline: Duration::from_millis(wire.candidate_deadline_ms),
            operation_deadline: Duration::from_millis(wire.operation_deadline_ms),
            shutdown_deadline: Duration::from_millis(wire.shutdown_deadline_ms),
            reopen_delay: Duration::from_millis(wire.reopen_delay_ms),
            thresholds: CaptureThresholds {
                minimum_fps_ratio: wire.minimum_fps_ratio,
                maximum_read_failure_ratio: wire.maximum_read_failure_ratio,
                maximum_gap_periods: wire.maximum_gap_periods,
                maximum_long_gap_ratio: wire.maximum_long_gap_ratio,
            },
        })
    }

    pub fn from_json(json: &str) -> Result<Self, ProfileConfigError> {
        let wire: ModeCandidatePolicyV1 =
            serde_json::from_str(json).map_err(|_| ProfileConfigError::CanonicalSerialization)?;
        Self::validate(wire)
    }

    pub fn candidates(&self) -> Vec<ModeTuple> {
        let mut candidates = Vec::with_capacity(self.candidate_count());
        for fourcc in &self.fourcc {
            for resolution in &self.resolutions {
                for requested_fps in &self.fps {
                    candidates.push(ModeTuple {
                        fourcc: *fourcc,
                        resolution: *resolution,
                        requested_fps: *requested_fps,
                    });
                }
            }
        }
        candidates
    }

    pub fn candidate_count(&self) -> usize {
        self.fourcc.len() * self.resolutions.len() * self.fps.len()
    }

    pub fn thresholds(&self) -> CaptureThresholds {
        self.thresholds
    }

    pub fn warmup(&self) -> Duration {
        self.warmup
    }

    pub fn capture_only(&self) -> Duration {
        self.capture_only
    }

    pub fn first_frame_deadline(&self) -> Duration {
        self.first_frame_deadline
    }

    pub fn candidate_deadline(&self) -> Duration {
        self.candidate_deadline
    }

    pub fn operation_deadline(&self) -> Duration {
        self.operation_deadline
    }

    pub fn shutdown_deadline(&self) -> Duration {
        self.shutdown_deadline
    }

    pub fn reopen_delay(&self) -> Duration {
        self.reopen_delay
    }

    pub fn canonical_hash(&self) -> Result<String, ProfileConfigError> {
        let bytes = serde_json::to_vec(&CanonicalPolicyHashV1::from(self))
            .map_err(|_| ProfileConfigError::CanonicalSerialization)?;
        let digest = Sha256::digest(bytes);
        Ok(format!("{digest:x}"))
    }

    pub fn to_wire(&self) -> ModeCandidatePolicyV1 {
        ModeCandidatePolicyV1 {
            schema_version: PROFILE_POLICY_SCHEMA_VERSION,
            fourcc: self.fourcc.iter().map(ToString::to_string).collect(),
            resolutions: self
                .resolutions
                .iter()
                .map(|value| ResolutionV1 {
                    width: value.width,
                    height: value.height,
                })
                .collect(),
            fps: self.fps.clone(),
            warmup_ms: duration_ms(self.warmup),
            capture_only_ms: duration_ms(self.capture_only),
            first_frame_deadline_ms: duration_ms(self.first_frame_deadline),
            candidate_deadline_ms: duration_ms(self.candidate_deadline),
            operation_deadline_ms: duration_ms(self.operation_deadline),
            shutdown_deadline_ms: duration_ms(self.shutdown_deadline),
            reopen_delay_ms: duration_ms(self.reopen_delay),
            minimum_fps_ratio: self.thresholds.minimum_fps_ratio,
            maximum_read_failure_ratio: self.thresholds.maximum_read_failure_ratio,
            maximum_gap_periods: self.thresholds.maximum_gap_periods,
            maximum_long_gap_ratio: self.thresholds.maximum_long_gap_ratio,
        }
    }
}

#[derive(Serialize)]
struct CanonicalPolicyHashV1 {
    hash_schema_version: u32,
    policy: ModeCandidatePolicyV1,
}

impl From<&ModeCandidatePolicy> for CanonicalPolicyHashV1 {
    fn from(policy: &ModeCandidatePolicy) -> Self {
        Self {
            hash_schema_version: PROFILE_HASH_SCHEMA_VERSION,
            policy: policy.to_wire(),
        }
    }
}

fn validate_list_size(
    field: &'static str,
    length: usize,
    limit: usize,
) -> Result<(), ProfileConfigError> {
    if length == 0 || length > limit {
        Err(ProfileConfigError::InvalidListSize { field, limit })
    } else {
        Ok(())
    }
}

fn require_unique<T: Eq + std::hash::Hash>(
    field: &'static str,
    values: impl IntoIterator<Item = T>,
) -> Result<(), ProfileConfigError> {
    let mut unique = HashSet::new();
    if values.into_iter().any(|value| !unique.insert(value)) {
        Err(ProfileConfigError::DuplicateValue { field })
    } else {
        Ok(())
    }
}

fn validate_ratio(
    field: &'static str,
    value: f64,
    zero_allowed: bool,
) -> Result<(), ProfileConfigError> {
    let lower_valid = if zero_allowed {
        value >= 0.0
    } else {
        value > 0.0
    };
    if !value.is_finite() || !lower_valid || value > 1.0 {
        Err(ProfileConfigError::InvalidThreshold { field })
    } else {
        Ok(())
    }
}

fn duration_ms(value: Duration) -> u64 {
    u64::try_from(value.as_millis()).unwrap_or(u64::MAX)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModeTuple {
    pub fourcc: FourCc,
    pub resolution: Resolution,
    pub requested_fps: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertySetDiagnostics {
    pub fourcc: bool,
    pub width: bool,
    pub height: bool,
    pub fps: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReportedCaptureProperties {
    pub fourcc: Option<FourCc>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub fps: Option<f64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameMetadata {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidatePhase {
    Opening,
    ApplyingProperties,
    ReadingReportedProperties,
    FirstFrame,
    Warmup,
    Measuring,
    Release,
    ReopenDelay,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CandidateFailureReason {
    OpenFailed,
    ApplyFailed,
    ReadFailed,
    ReleaseFailed,
    FirstFrameTimeout,
    ReadStalled,
    CandidateDeadline,
    OperationDeadline,
    Cancelled,
    InsufficientFrames,
    CoercedResolution,
    UnderTargetFps,
    ReadFailureRatio,
    MaximumGap,
    LongGapRatio,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureModeStatus {
    OpeningFailed,
    ApplyingFailed,
    ReadFailed,
    ReleaseFailed,
    FirstFrameTimeout,
    ReadStalled,
    CandidateTimedOut,
    OperationTimedOut,
    CoercedResolution,
    CaptureUnderTarget,
    CaptureUnstable,
    VerifiedFourCcReportedMatch,
    VerifiedFourCcUnconfirmed,
    Cancelled,
}

impl CaptureModeStatus {
    pub fn is_verified(self) -> bool {
        matches!(
            self,
            Self::VerifiedFourCcReportedMatch | Self::VerifiedFourCcUnconfirmed
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureMetrics {
    pub elapsed: Duration,
    pub read_attempts: u64,
    pub captured_frames: u64,
    pub empty_frames: u64,
    pub read_failures: u64,
    pub measured_fps: f64,
    pub median_interval: Option<Duration>,
    pub p95_interval: Option<Duration>,
    pub p99_interval: Option<Duration>,
    pub maximum_gap: Option<Duration>,
    pub long_gap_count: u64,
    pub long_gap_ratio: f64,
    pub actual_resolutions: Vec<Resolution>,
}

impl CaptureMetrics {
    pub fn empty() -> Self {
        Self {
            elapsed: Duration::ZERO,
            read_attempts: 0,
            captured_frames: 0,
            empty_frames: 0,
            read_failures: 0,
            measured_fps: 0.0,
            median_interval: None,
            p95_interval: None,
            p99_interval: None,
            maximum_gap: None,
            long_gap_count: 0,
            long_gap_ratio: 0.0,
            actual_resolutions: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum MetricsError {
    #[error("measurement timestamp moved backwards")]
    NonMonotonicTimestamp,
    #[error("measurement sample limit exceeded")]
    SampleLimitExceeded,
    #[error("elapsed measurement time must be positive")]
    ZeroElapsed,
    #[error("requested FPS must be finite and positive")]
    InvalidRequestedFps,
}

#[derive(Default)]
pub struct CaptureMetricsAccumulator {
    read_attempts: u64,
    empty_frames: u64,
    read_failures: u64,
    frame_timestamps: Vec<Duration>,
    actual_resolutions: Vec<Resolution>,
}

impl CaptureMetricsAccumulator {
    pub fn note_empty(&mut self) {
        self.read_attempts = self.read_attempts.saturating_add(1);
        self.empty_frames = self.empty_frames.saturating_add(1);
    }

    pub fn note_failure(&mut self) {
        self.read_attempts = self.read_attempts.saturating_add(1);
        self.read_failures = self.read_failures.saturating_add(1);
    }

    pub fn note_frame(
        &mut self,
        timestamp: Duration,
        metadata: FrameMetadata,
    ) -> Result<(), MetricsError> {
        if self.frame_timestamps.len() >= MAX_METRIC_SAMPLES {
            return Err(MetricsError::SampleLimitExceeded);
        }
        if self
            .frame_timestamps
            .last()
            .is_some_and(|previous| timestamp < *previous)
        {
            return Err(MetricsError::NonMonotonicTimestamp);
        }
        self.read_attempts = self.read_attempts.saturating_add(1);
        self.frame_timestamps.push(timestamp);
        self.actual_resolutions.push(Resolution {
            width: metadata.width,
            height: metadata.height,
        });
        Ok(())
    }

    pub fn finish(
        self,
        elapsed: Duration,
        requested_fps: f64,
    ) -> Result<CaptureMetrics, MetricsError> {
        if elapsed.is_zero() {
            return Err(MetricsError::ZeroElapsed);
        }
        if !requested_fps.is_finite() || requested_fps <= 0.0 {
            return Err(MetricsError::InvalidRequestedFps);
        }
        let captured_frames = u64::try_from(self.frame_timestamps.len())
            .map_err(|_| MetricsError::SampleLimitExceeded)?;
        let mut intervals = self
            .frame_timestamps
            .windows(2)
            .map(|pair| pair[1].saturating_sub(pair[0]))
            .collect::<Vec<_>>();
        intervals.sort_unstable();
        let median_interval = median(&intervals);
        let p95_interval = nearest_rank(&intervals, 95);
        let p99_interval = nearest_rank(&intervals, 99);
        let maximum_gap = intervals.last().copied();
        let nominal_seconds = 1.0 / requested_fps;
        let long_gap_count = intervals
            .iter()
            .filter(|value| value.as_secs_f64() > 2.0 * nominal_seconds)
            .count() as u64;
        let long_gap_ratio = if intervals.is_empty() {
            0.0
        } else {
            long_gap_count as f64 / intervals.len() as f64
        };
        Ok(CaptureMetrics {
            elapsed,
            read_attempts: self.read_attempts,
            captured_frames,
            empty_frames: self.empty_frames,
            read_failures: self.read_failures,
            measured_fps: captured_frames as f64 / elapsed.as_secs_f64(),
            median_interval,
            p95_interval,
            p99_interval,
            maximum_gap,
            long_gap_count,
            long_gap_ratio,
            actual_resolutions: self.actual_resolutions,
        })
    }
}

fn median(values: &[Duration]) -> Option<Duration> {
    let middle = values.len() / 2;
    if values.is_empty() {
        None
    } else if values.len() % 2 == 1 {
        Some(values[middle])
    } else {
        let left = values[middle - 1].as_nanos();
        let right = values[middle].as_nanos();
        let average = left / 2 + right / 2 + (left % 2 + right % 2) / 2;
        Some(Duration::from_nanos(
            u64::try_from(average).unwrap_or(u64::MAX),
        ))
    }
}

fn nearest_rank(values: &[Duration], percentile: usize) -> Option<Duration> {
    if values.is_empty() {
        return None;
    }
    let rank = values.len().saturating_mul(percentile).div_ceil(100);
    values.get(rank.saturating_sub(1)).copied()
}

#[derive(Clone, Debug, PartialEq)]
pub struct GateEvaluation {
    pub status: CaptureModeStatus,
    pub failure_reasons: Vec<CandidateFailureReason>,
}

pub fn evaluate_capture_gates(
    tuple: ModeTuple,
    reported: ReportedCaptureProperties,
    metrics: &CaptureMetrics,
    thresholds: CaptureThresholds,
) -> GateEvaluation {
    let mut reasons = Vec::new();
    if metrics.captured_frames < 2 {
        reasons.push(CandidateFailureReason::InsufficientFrames);
    }
    if metrics
        .actual_resolutions
        .iter()
        .any(|actual| *actual != tuple.resolution)
    {
        reasons.push(CandidateFailureReason::CoercedResolution);
    }
    if metrics.measured_fps < tuple.requested_fps * thresholds.minimum_fps_ratio {
        reasons.push(CandidateFailureReason::UnderTargetFps);
    }
    let failure_ratio = if metrics.read_attempts == 0 {
        1.0
    } else {
        metrics.read_failures as f64 / metrics.read_attempts as f64
    };
    if failure_ratio > thresholds.maximum_read_failure_ratio {
        reasons.push(CandidateFailureReason::ReadFailureRatio);
    }
    let nominal_period = Duration::from_secs_f64(1.0 / tuple.requested_fps);
    let maximum_allowed_gap = nominal_period.mul_f64(thresholds.maximum_gap_periods);
    if metrics
        .maximum_gap
        .is_some_and(|gap| gap > maximum_allowed_gap)
    {
        reasons.push(CandidateFailureReason::MaximumGap);
    }
    if metrics.long_gap_ratio > thresholds.maximum_long_gap_ratio {
        reasons.push(CandidateFailureReason::LongGapRatio);
    }

    let status = if reasons.contains(&CandidateFailureReason::CoercedResolution) {
        CaptureModeStatus::CoercedResolution
    } else if reasons.contains(&CandidateFailureReason::UnderTargetFps)
        || reasons.contains(&CandidateFailureReason::InsufficientFrames)
    {
        CaptureModeStatus::CaptureUnderTarget
    } else if !reasons.is_empty() {
        CaptureModeStatus::CaptureUnstable
    } else if reported.fourcc == Some(tuple.fourcc) {
        CaptureModeStatus::VerifiedFourCcReportedMatch
    } else {
        CaptureModeStatus::VerifiedFourCcUnconfirmed
    };
    GateEvaluation {
        status,
        failure_reasons: reasons,
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ProfileId(String);

impl ProfileId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn issued(service_instance: u64, generation: u64) -> Self {
        Self(format!("profile-{service_instance:016x}-{generation:016x}"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileOperationStatus {
    Profiling,
    Completed,
    Cancelled,
    Failed,
    Stuck,
}

impl ProfileOperationStatus {
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Profiling)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProfileSnapshot {
    pub profile_id: ProfileId,
    pub service_state: crate::camera::CameraServiceState,
    pub status: ProfileOperationStatus,
    pub endpoint_key: DeviceEndpointKey,
    pub scan_generation: u64,
    pub backend: CaptureBackend,
    pub policy: ModeCandidatePolicyV1,
    pub config_hash: String,
    pub completed_candidates: usize,
    pub total_candidates: usize,
    pub current_candidate: Option<ModeTuple>,
    pub current_phase: Option<CandidatePhase>,
    pub failure_reason: Option<CandidateFailureReason>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct VerifiedModeId(String);

impl VerifiedModeId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CandidateResult {
    pub ordinal: usize,
    pub attempt_count: u8,
    pub retry_reason: Option<CandidateRetryReason>,
    pub tuple: ModeTuple,
    pub set_diagnostics: PropertySetDiagnostics,
    pub reported: ReportedCaptureProperties,
    pub metrics: CaptureMetrics,
    pub status: CaptureModeStatus,
    pub failure_reasons: Vec<CandidateFailureReason>,
    verified_mode_id: Option<VerifiedModeId>,
}

impl CandidateResult {
    pub fn from_terminal_gate(
        ordinal: usize,
        tuple: ModeTuple,
        set_diagnostics: PropertySetDiagnostics,
        reported: ReportedCaptureProperties,
        metrics: CaptureMetrics,
        evaluation: GateEvaluation,
    ) -> Self {
        Self {
            ordinal,
            attempt_count: 1,
            retry_reason: None,
            tuple,
            set_diagnostics,
            reported,
            metrics,
            status: evaluation.status,
            failure_reasons: evaluation.failure_reasons,
            verified_mode_id: None,
        }
    }

    pub fn verified_mode_id(&self) -> Option<&VerifiedModeId> {
        self.verified_mode_id.as_ref()
    }

    pub fn failed(
        ordinal: usize,
        tuple: ModeTuple,
        attempt_count: u8,
        retry_reason: Option<CandidateRetryReason>,
        status: CaptureModeStatus,
        failure_reasons: Vec<CandidateFailureReason>,
    ) -> Self {
        debug_assert!(!status.is_verified());
        Self {
            ordinal,
            attempt_count,
            retry_reason,
            tuple,
            set_diagnostics: PropertySetDiagnostics {
                fourcc: false,
                width: false,
                height: false,
                fps: false,
            },
            reported: ReportedCaptureProperties {
                fourcc: None,
                width: None,
                height: None,
                fps: None,
            },
            metrics: CaptureMetrics::empty(),
            status,
            failure_reasons,
            verified_mode_id: None,
        }
    }

    pub fn set_attempt(&mut self, attempt_count: u8, retry_reason: Option<CandidateRetryReason>) {
        self.attempt_count = attempt_count;
        self.retry_reason = retry_reason;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateRetryReason {
    OpenFailed,
    FirstReadStartFailed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VerifiedModeDescriptor {
    pub profile_id: ProfileId,
    pub endpoint_key: DeviceEndpointKey,
    pub scan_generation: u64,
    pub backend: CaptureBackend,
    pub tuple: ModeTuple,
    pub config_hash: String,
}

#[derive(Default)]
pub struct VerifiedModeRegistry {
    generation: u64,
    entries: HashMap<VerifiedModeId, VerifiedModeDescriptor>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum VerifiedModeError {
    #[error("verified mode is stale")]
    Stale,
    #[error("actual resolution was coerced")]
    ModeCoerced,
    #[error("live FPS is under target")]
    UnderTargetFps,
}

impl VerifiedModeRegistry {
    pub fn issue(
        &mut self,
        result: &mut CandidateResult,
        descriptor: VerifiedModeDescriptor,
    ) -> Option<VerifiedModeId> {
        if !result.status.is_verified() || descriptor.tuple != result.tuple {
            return None;
        }
        let id = VerifiedModeId(format!(
            "verified-{:016x}-{}-{:04x}",
            self.generation,
            descriptor.profile_id.as_str(),
            result.ordinal
        ));
        self.entries.insert(id.clone(), descriptor);
        result.verified_mode_id = Some(id.clone());
        Some(id)
    }

    pub fn lookup(
        &self,
        id: &VerifiedModeId,
        active_profile: &ProfileId,
        endpoint_key: &DeviceEndpointKey,
        scan_generation: u64,
        config_hash: &str,
    ) -> Result<&VerifiedModeDescriptor, VerifiedModeError> {
        let descriptor = self.entries.get(id).ok_or(VerifiedModeError::Stale)?;
        if &descriptor.profile_id != active_profile
            || &descriptor.endpoint_key != endpoint_key
            || descriptor.scan_generation != scan_generation
            || descriptor.config_hash != config_hash
        {
            return Err(VerifiedModeError::Stale);
        }
        Ok(descriptor)
    }

    pub fn invalidate(&mut self) {
        self.entries.clear();
        self.generation = self.generation.saturating_add(1);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolutionMaximum {
    pub resolution: Resolution,
    pub result_ordinals: Vec<usize>,
    pub measured_fps: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileTerminalStatus {
    Completed,
    Cancelled,
    Failed,
    Stuck,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentVersionReferences {
    pub app_version: String,
    pub rust_version: String,
    pub tauri_version: String,
    pub opencv_version: String,
    pub opencv_crate_version: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProfileReport {
    pub profile_id: ProfileId,
    pub started_at_unix_ms: u64,
    pub environment: EnvironmentVersionReferences,
    pub endpoint_key: DeviceEndpointKey,
    pub scan_generation: u64,
    pub backend: CaptureBackend,
    pub policy: ModeCandidatePolicyV1,
    pub config_hash: String,
    pub status: ProfileTerminalStatus,
    pub failure_reason: Option<CandidateFailureReason>,
    pub results: Vec<CandidateResult>,
    pub max_verified_by_resolution: Vec<ResolutionMaximum>,
}

pub struct ProfileReportInput {
    pub profile_id: ProfileId,
    pub started_at_unix_ms: u64,
    pub environment: EnvironmentVersionReferences,
    pub endpoint_key: DeviceEndpointKey,
    pub scan_generation: u64,
    pub backend: CaptureBackend,
    pub policy: ModeCandidatePolicy,
    pub status: ProfileTerminalStatus,
    pub failure_reason: Option<CandidateFailureReason>,
    pub results: Vec<CandidateResult>,
}

impl ProfileReport {
    pub fn finalize(input: ProfileReportInput) -> Result<Self, ProfileConfigError> {
        let config_hash = input.policy.canonical_hash()?;
        let max_verified_by_resolution = max_verified_by_resolution(&input.results);
        Ok(Self {
            profile_id: input.profile_id,
            started_at_unix_ms: input.started_at_unix_ms,
            environment: input.environment,
            endpoint_key: input.endpoint_key,
            scan_generation: input.scan_generation,
            backend: input.backend,
            policy: input.policy.to_wire(),
            config_hash,
            status: input.status,
            failure_reason: input.failure_reason,
            results: input.results,
            max_verified_by_resolution,
        })
    }
}

pub fn max_verified_by_resolution(results: &[CandidateResult]) -> Vec<ResolutionMaximum> {
    let mut maxima: Vec<ResolutionMaximum> = Vec::new();
    for result in results.iter().filter(|result| result.status.is_verified()) {
        match maxima
            .iter_mut()
            .find(|maximum| maximum.resolution == result.tuple.resolution)
        {
            Some(maximum) if result.metrics.measured_fps > maximum.measured_fps => {
                maximum.measured_fps = result.metrics.measured_fps;
                maximum.result_ordinals = vec![result.ordinal];
            }
            Some(maximum) if result.metrics.measured_fps == maximum.measured_fps => {
                maximum.result_ordinals.push(result.ordinal);
            }
            Some(_) => {}
            None => maxima.push(ResolutionMaximum {
                resolution: result.tuple.resolution,
                result_ordinals: vec![result.ordinal],
                measured_fps: result.metrics.measured_fps,
            }),
        }
    }
    maxima
}

pub fn revalidate_verified_mode(
    descriptor: &VerifiedModeDescriptor,
    actual_resolution: Resolution,
    measured_fps: f64,
    minimum_fps_ratio: f64,
) -> Result<(), VerifiedModeError> {
    if actual_resolution != descriptor.tuple.resolution {
        return Err(VerifiedModeError::ModeCoerced);
    }
    if measured_fps < descriptor.tuple.requested_fps * minimum_fps_ratio {
        return Err(VerifiedModeError::UnderTargetFps);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_JSON: &str = include_str!("../../../config/mode-candidates.json");

    fn default_wire() -> ModeCandidatePolicyV1 {
        serde_json::from_str(DEFAULT_JSON).expect("valid fixture JSON")
    }

    fn policy() -> ModeCandidatePolicy {
        ModeCandidatePolicy::validate(default_wire()).expect("valid default policy")
    }

    #[test]
    fn default_policy_is_valid_and_bounded() {
        let policy = policy();
        assert_eq!(policy.candidate_count(), 30);
        assert_eq!(policy.candidates().len(), 30);
    }

    #[test]
    fn policy_rejects_schema_lists_duplicates_and_boundaries() {
        let mut wire = default_wire();
        wire.schema_version = 2;
        assert_eq!(
            ModeCandidatePolicy::validate(wire),
            Err(ProfileConfigError::UnsupportedSchema)
        );

        let mut wire = default_wire();
        wire.fourcc.clear();
        assert!(matches!(
            ModeCandidatePolicy::validate(wire),
            Err(ProfileConfigError::InvalidListSize {
                field: "fourcc",
                ..
            })
        ));

        let mut wire = default_wire();
        wire.fourcc[0] = "MJG".into();
        assert_eq!(
            ModeCandidatePolicy::validate(wire),
            Err(ProfileConfigError::InvalidFourCc)
        );

        let mut wire = default_wire();
        wire.fourcc.push("MJPG".into());
        assert!(matches!(
            ModeCandidatePolicy::validate(wire),
            Err(ProfileConfigError::DuplicateValue { field: "FourCC" })
        ));

        let mut wire = default_wire();
        wire.resolutions[0].width = 0;
        assert_eq!(
            ModeCandidatePolicy::validate(wire),
            Err(ProfileConfigError::InvalidResolution)
        );

        let mut wire = default_wire();
        wire.fps[0] = f64::NAN;
        assert_eq!(
            ModeCandidatePolicy::validate(wire),
            Err(ProfileConfigError::InvalidFps)
        );

        let mut wire = default_wire();
        wire.fps[1] = wire.fps[0];
        assert!(matches!(
            ModeCandidatePolicy::validate(wire),
            Err(ProfileConfigError::DuplicateValue { field: "FPS" })
        ));

        let mut wire = default_wire();
        wire.warmup_ms = 0;
        assert!(matches!(
            ModeCandidatePolicy::validate(wire),
            Err(ProfileConfigError::InvalidDuration { field: "warmup_ms" })
        ));

        let mut wire = default_wire();
        wire.minimum_fps_ratio = f64::INFINITY;
        assert!(matches!(
            ModeCandidatePolicy::validate(wire),
            Err(ProfileConfigError::InvalidThreshold {
                field: "minimum_fps_ratio"
            })
        ));

        let mut wire = default_wire();
        wire.candidate_deadline_ms = 1;
        assert_eq!(
            ModeCandidatePolicy::validate(wire),
            Err(ProfileConfigError::InconsistentDeadlines)
        );
    }

    #[test]
    fn policy_rejects_candidate_limit_before_plan_allocation() {
        let mut wire = default_wire();
        wire.fourcc = (0..8).map(|value| format!("A{value:03}")).collect();
        wire.resolutions = (1..=16)
            .map(|value| ResolutionV1 {
                width: value,
                height: value,
            })
            .collect();
        wire.fps = (1..=16).map(f64::from).collect();
        assert_eq!(
            ModeCandidatePolicy::validate(wire),
            Err(ProfileConfigError::TooManyCandidates)
        );
    }

    #[test]
    fn policy_checks_all_numeric_boundaries_duplicates_and_deadline_relations() {
        let mut wire = default_wire();
        wire.resolutions.push(wire.resolutions[0]);
        assert!(matches!(
            ModeCandidatePolicy::validate(wire),
            Err(ProfileConfigError::DuplicateValue {
                field: "resolution"
            })
        ));

        for invalid in [0, MAX_DIMENSION + 1] {
            let mut wire = default_wire();
            wire.resolutions[0].height = invalid;
            assert_eq!(
                ModeCandidatePolicy::validate(wire),
                Err(ProfileConfigError::InvalidResolution)
            );
        }
        for invalid in [0.0, -1.0, MAX_FPS + 1.0, f64::INFINITY, f64::NAN] {
            let mut wire = default_wire();
            wire.fps[0] = invalid;
            assert_eq!(
                ModeCandidatePolicy::validate(wire),
                Err(ProfileConfigError::InvalidFps)
            );
        }

        for field in [
            "warmup_ms",
            "capture_only_ms",
            "first_frame_deadline_ms",
            "candidate_deadline_ms",
            "operation_deadline_ms",
            "shutdown_deadline_ms",
            "reopen_delay_ms",
        ] {
            for invalid in [0, MAX_DURATION_MS + 1] {
                let mut wire = default_wire();
                match field {
                    "warmup_ms" => wire.warmup_ms = invalid,
                    "capture_only_ms" => wire.capture_only_ms = invalid,
                    "first_frame_deadline_ms" => wire.first_frame_deadline_ms = invalid,
                    "candidate_deadline_ms" => wire.candidate_deadline_ms = invalid,
                    "operation_deadline_ms" => wire.operation_deadline_ms = invalid,
                    "shutdown_deadline_ms" => wire.shutdown_deadline_ms = invalid,
                    "reopen_delay_ms" => wire.reopen_delay_ms = invalid,
                    _ => unreachable!(),
                }
                assert!(matches!(
                    ModeCandidatePolicy::validate(wire),
                    Err(ProfileConfigError::InvalidDuration { .. })
                ));
            }
        }

        for invalid in [0.0, -0.1, 1.1, f64::NAN] {
            let mut wire = default_wire();
            wire.minimum_fps_ratio = invalid;
            assert!(matches!(
                ModeCandidatePolicy::validate(wire),
                Err(ProfileConfigError::InvalidThreshold {
                    field: "minimum_fps_ratio"
                })
            ));
        }
        for invalid in [-0.1, 1.1, f64::INFINITY] {
            let mut wire = default_wire();
            wire.maximum_read_failure_ratio = invalid;
            assert!(matches!(
                ModeCandidatePolicy::validate(wire),
                Err(ProfileConfigError::InvalidThreshold {
                    field: "maximum_read_failure_ratio"
                })
            ));
            let mut wire = default_wire();
            wire.maximum_long_gap_ratio = invalid;
            assert!(matches!(
                ModeCandidatePolicy::validate(wire),
                Err(ProfileConfigError::InvalidThreshold {
                    field: "maximum_long_gap_ratio"
                })
            ));
        }
        for invalid in [0.99, f64::NEG_INFINITY, f64::NAN] {
            let mut wire = default_wire();
            wire.maximum_gap_periods = invalid;
            assert!(matches!(
                ModeCandidatePolicy::validate(wire),
                Err(ProfileConfigError::InvalidThreshold {
                    field: "maximum_gap_periods"
                })
            ));
        }

        let mut wire = default_wire();
        wire.candidate_deadline_ms =
            wire.first_frame_deadline_ms + wire.warmup_ms + wire.capture_only_ms - 1;
        assert_eq!(
            ModeCandidatePolicy::validate(wire),
            Err(ProfileConfigError::InconsistentDeadlines)
        );
        let mut wire = default_wire();
        wire.operation_deadline_ms = wire.candidate_deadline_ms - 1;
        assert_eq!(
            ModeCandidatePolicy::validate(wire),
            Err(ProfileConfigError::InconsistentDeadlines)
        );
        let mut wire = default_wire();
        wire.shutdown_deadline_ms = wire.candidate_deadline_ms + 1;
        assert_eq!(
            ModeCandidatePolicy::validate(wire),
            Err(ProfileConfigError::InconsistentDeadlines)
        );
    }

    #[test]
    fn canonical_hash_ignores_source_format_and_key_order_but_not_array_order() {
        let compact = serde_json::to_string(&default_wire()).expect("serialize");
        let reversed_keys = "{\"maximum_long_gap_ratio\":0.01,\"maximum_gap_periods\":5.0,\"maximum_read_failure_ratio\":0.01,\"minimum_fps_ratio\":0.95,\"reopen_delay_ms\":500,\"shutdown_deadline_ms\":3000,\"operation_deadline_ms\":600000,\"candidate_deadline_ms\":60000,\"first_frame_deadline_ms\":5000,\"capture_only_ms\":10000,\"warmup_ms\":3000,\"fps\":[120.0,60.0,59.94,30.0,29.97],\"resolutions\":[{\"height\":480,\"width\":640},{\"height\":720,\"width\":1280},{\"height\":1080,\"width\":1920}],\"fourcc\":[\"MJPG\",\"YUY2\"],\"schema_version\":1}";
        let first = ModeCandidatePolicy::from_json(&compact).expect("compact");
        let second = ModeCandidatePolicy::from_json(reversed_keys).expect("reordered");
        assert_eq!(
            first.canonical_hash().expect("hash"),
            second.canonical_hash().expect("hash")
        );
        assert_eq!(
            first.canonical_hash().expect("hash"),
            "d0547bff91b14df3aec3e390964e0f46d63ca82d8634f14d57eb60bfb0a0d53c"
        );

        let mut changed = default_wire();
        changed.fourcc.reverse();
        let changed = ModeCandidatePolicy::validate(changed).expect("valid reordered arrays");
        assert_ne!(
            first.canonical_hash().expect("hash"),
            changed.canonical_hash().expect("hash")
        );
    }

    #[test]
    fn cartesian_product_preserves_exact_policy_order() {
        let candidates = policy().candidates();
        assert_eq!(candidates[0].fourcc.as_str(), "MJPG");
        assert_eq!(
            candidates[0].resolution,
            Resolution {
                width: 640,
                height: 480
            }
        );
        assert_eq!(candidates[0].requested_fps, 120.0);
        assert_eq!(candidates[4].requested_fps, 29.97);
        assert_eq!(
            candidates[5].resolution,
            Resolution {
                width: 1280,
                height: 720
            }
        );
        assert_eq!(candidates[15].fourcc.as_str(), "YUY2");
    }

    #[test]
    fn metrics_include_delayed_first_frame_and_use_nearest_rank() {
        let mut accumulator = CaptureMetricsAccumulator::default();
        accumulator.note_empty();
        accumulator.note_failure();
        for millis in [100, 110, 130, 160, 200] {
            accumulator
                .note_frame(
                    Duration::from_millis(millis),
                    FrameMetadata {
                        width: 640,
                        height: 480,
                    },
                )
                .expect("sample");
        }
        let metrics = accumulator
            .finish(Duration::from_millis(250), 30.0)
            .expect("metrics");
        assert_eq!(metrics.read_attempts, 7);
        assert_eq!(metrics.captured_frames, 5);
        assert_eq!(metrics.measured_fps, 20.0);
        assert_eq!(metrics.median_interval, Some(Duration::from_millis(25)));
        assert_eq!(metrics.p95_interval, Some(Duration::from_millis(40)));
        assert_eq!(metrics.p99_interval, Some(Duration::from_millis(40)));
        assert_eq!(metrics.maximum_gap, Some(Duration::from_millis(40)));
    }

    #[test]
    fn metrics_define_zero_attempt_and_less_than_two_frame_behavior() {
        let empty = CaptureMetricsAccumulator::default()
            .finish(Duration::from_secs(1), 30.0)
            .expect("empty metrics");
        assert_eq!(empty.measured_fps, 0.0);
        assert_eq!(empty.median_interval, None);
        let evaluation = evaluate_capture_gates(
            policy().candidates()[0],
            ReportedCaptureProperties {
                fourcc: None,
                width: None,
                height: None,
                fps: None,
            },
            &empty,
            policy().thresholds(),
        );
        assert!(
            evaluation
                .failure_reasons
                .contains(&CandidateFailureReason::InsufficientFrames)
        );
        assert!(
            evaluation
                .failure_reasons
                .contains(&CandidateFailureReason::ReadFailureRatio)
        );
    }

    #[test]
    fn metrics_enforce_sample_bound() {
        let mut accumulator = CaptureMetricsAccumulator::default();
        for index in 0..MAX_METRIC_SAMPLES {
            accumulator
                .note_frame(
                    Duration::from_nanos(index as u64),
                    FrameMetadata {
                        width: 1,
                        height: 1,
                    },
                )
                .expect("bounded sample");
        }
        assert_eq!(
            accumulator.note_frame(
                Duration::from_secs(1),
                FrameMetadata {
                    width: 1,
                    height: 1
                }
            ),
            Err(MetricsError::SampleLimitExceeded)
        );
    }

    #[test]
    fn gate_collects_all_failures_and_ignores_set_diagnostics() {
        let tuple = policy().candidates()[0];
        let mut accumulator = CaptureMetricsAccumulator::default();
        accumulator.note_failure();
        accumulator
            .note_frame(
                Duration::from_millis(900),
                FrameMetadata {
                    width: 320,
                    height: 240,
                },
            )
            .expect("frame");
        let metrics = accumulator
            .finish(Duration::from_secs(1), tuple.requested_fps)
            .expect("metrics");
        let evaluation = evaluate_capture_gates(
            tuple,
            ReportedCaptureProperties {
                fourcc: Some(tuple.fourcc),
                width: Some(640.0),
                height: Some(480.0),
                fps: Some(120.0),
            },
            &metrics,
            policy().thresholds(),
        );
        assert_eq!(evaluation.status, CaptureModeStatus::CoercedResolution);
        assert!(
            evaluation
                .failure_reasons
                .contains(&CandidateFailureReason::CoercedResolution)
        );
        assert!(
            evaluation
                .failure_reasons
                .contains(&CandidateFailureReason::UnderTargetFps)
        );
        assert!(
            evaluation
                .failure_reasons
                .contains(&CandidateFailureReason::ReadFailureRatio)
        );
    }

    #[test]
    fn set_false_can_verify_and_set_true_does_not_hide_coercion() {
        let policy = policy();
        let tuple = policy.candidates()[0];
        let passing_metrics = metrics_for(tuple.resolution, 120.0);
        let reported = ReportedCaptureProperties {
            fourcc: Some(tuple.fourcc),
            width: Some(640.0),
            height: Some(480.0),
            fps: Some(120.0),
        };
        let passing =
            evaluate_capture_gates(tuple, reported, &passing_metrics, policy.thresholds());
        let result = CandidateResult::from_terminal_gate(
            0,
            tuple,
            PropertySetDiagnostics {
                fourcc: false,
                width: false,
                height: false,
                fps: false,
            },
            reported,
            passing_metrics,
            passing,
        );
        assert!(result.status.is_verified());

        let coerced_metrics = metrics_for(
            Resolution {
                width: 320,
                height: 240,
            },
            120.0,
        );
        let coerced =
            evaluate_capture_gates(tuple, reported, &coerced_metrics, policy.thresholds());
        assert_eq!(coerced.status, CaptureModeStatus::CoercedResolution);
    }

    #[test]
    fn registry_issues_only_after_verified_gate_and_invalidates_stale_ids() {
        let policy = policy();
        let tuple = policy.candidates()[0];
        let mut accumulator = CaptureMetricsAccumulator::default();
        for millis in [0, 8, 16, 24, 32] {
            accumulator
                .note_frame(
                    Duration::from_millis(millis),
                    FrameMetadata {
                        width: 640,
                        height: 480,
                    },
                )
                .expect("frame");
        }
        let metrics = accumulator
            .finish(Duration::from_millis(40), tuple.requested_fps)
            .expect("metrics");
        let reported = ReportedCaptureProperties {
            fourcc: Some(tuple.fourcc),
            width: Some(640.0),
            height: Some(480.0),
            fps: Some(120.0),
        };
        let evaluation = evaluate_capture_gates(tuple, reported, &metrics, policy.thresholds());
        assert!(evaluation.status.is_verified());
        let mut result = CandidateResult::from_terminal_gate(
            0,
            tuple,
            PropertySetDiagnostics {
                fourcc: false,
                width: false,
                height: false,
                fps: false,
            },
            reported,
            metrics,
            evaluation,
        );
        let profile_id = ProfileId::new("profile-1");
        let endpoint = DeviceEndpointKey("endpoint-test".into());
        let descriptor = VerifiedModeDescriptor {
            profile_id: profile_id.clone(),
            endpoint_key: endpoint.clone(),
            scan_generation: 1,
            backend: CaptureBackend::Dshow,
            tuple,
            config_hash: "hash".into(),
        };
        let mut registry = VerifiedModeRegistry::default();
        let id = registry
            .issue(&mut result, descriptor)
            .expect("verified id");
        assert!(
            registry
                .lookup(&id, &profile_id, &endpoint, 1, "hash")
                .is_ok()
        );
        assert_eq!(
            registry.lookup(&id, &ProfileId::new("other"), &endpoint, 1, "hash"),
            Err(VerifiedModeError::Stale)
        );
        assert_eq!(
            registry.lookup(&id, &profile_id, &endpoint, 2, "hash"),
            Err(VerifiedModeError::Stale)
        );
        assert_eq!(
            registry.lookup(&id, &profile_id, &endpoint, 1, "other-hash"),
            Err(VerifiedModeError::Stale)
        );
        registry.invalidate();
        assert_eq!(
            registry.lookup(&id, &profile_id, &endpoint, 1, "hash"),
            Err(VerifiedModeError::Stale)
        );
    }

    #[test]
    fn aggregation_keeps_ties_and_excludes_rejected_results() {
        let policy = policy();
        let tuples = policy.candidates();
        let make = |ordinal, tuple, fps, status| CandidateResult {
            ordinal,
            attempt_count: 1,
            retry_reason: None,
            tuple,
            set_diagnostics: PropertySetDiagnostics {
                fourcc: true,
                width: true,
                height: true,
                fps: true,
            },
            reported: ReportedCaptureProperties {
                fourcc: Some(tuple.fourcc),
                width: None,
                height: None,
                fps: None,
            },
            metrics: CaptureMetrics {
                elapsed: Duration::from_secs(1),
                read_attempts: 2,
                captured_frames: 2,
                empty_frames: 0,
                read_failures: 0,
                measured_fps: fps,
                median_interval: Some(Duration::from_millis(10)),
                p95_interval: Some(Duration::from_millis(10)),
                p99_interval: Some(Duration::from_millis(10)),
                maximum_gap: Some(Duration::from_millis(10)),
                long_gap_count: 0,
                long_gap_ratio: 0.0,
                actual_resolutions: vec![tuple.resolution; 2],
            },
            status,
            failure_reasons: Vec::new(),
            verified_mode_id: None,
        };
        let results = vec![
            make(
                0,
                tuples[0],
                100.0,
                CaptureModeStatus::VerifiedFourCcReportedMatch,
            ),
            make(1, tuples[1], 130.0, CaptureModeStatus::CaptureUnderTarget),
            make(
                2,
                tuples[15],
                100.0,
                CaptureModeStatus::VerifiedFourCcUnconfirmed,
            ),
        ];
        let maxima = max_verified_by_resolution(&results);
        assert_eq!(maxima.len(), 1);
        assert_eq!(maxima[0].result_ordinals, vec![0, 2]);
    }

    #[test]
    fn report_finalization_keeps_one_endpoint_and_terminal_aggregation() {
        let policy = policy();
        let tuple = policy.candidates()[0];
        let metrics = metrics_for(tuple.resolution, 120.0);
        let reported = ReportedCaptureProperties {
            fourcc: Some(tuple.fourcc),
            width: Some(640.0),
            height: Some(480.0),
            fps: Some(120.0),
        };
        let evaluation = evaluate_capture_gates(tuple, reported, &metrics, policy.thresholds());
        let result = CandidateResult::from_terminal_gate(
            0,
            tuple,
            PropertySetDiagnostics {
                fourcc: true,
                width: true,
                height: true,
                fps: true,
            },
            reported,
            metrics,
            evaluation,
        );
        let report = ProfileReport::finalize(ProfileReportInput {
            profile_id: ProfileId::new("profile-1"),
            started_at_unix_ms: 1,
            environment: EnvironmentVersionReferences {
                app_version: "0.1.0".into(),
                rust_version: "1.98.1".into(),
                tauri_version: "2.11.5".into(),
                opencv_version: "4.12.0".into(),
                opencv_crate_version: "0.100.1".into(),
            },
            endpoint_key: DeviceEndpointKey("endpoint-a".into()),
            scan_generation: 1,
            backend: CaptureBackend::Dshow,
            policy,
            status: ProfileTerminalStatus::Completed,
            failure_reason: None,
            results: vec![result],
        })
        .expect("report");
        assert_eq!(report.endpoint_key.as_str(), "endpoint-a");
        assert_eq!(report.max_verified_by_resolution.len(), 1);
        assert_eq!(
            report.max_verified_by_resolution[0].result_ordinals,
            vec![0]
        );
    }

    #[test]
    fn revalidation_rejects_coercion_and_under_target_fps() {
        let tuple = policy().candidates()[0];
        let descriptor = VerifiedModeDescriptor {
            profile_id: ProfileId::new("p"),
            endpoint_key: DeviceEndpointKey("e".into()),
            scan_generation: 1,
            backend: CaptureBackend::Dshow,
            tuple,
            config_hash: "h".into(),
        };
        assert_eq!(
            revalidate_verified_mode(
                &descriptor,
                Resolution {
                    width: 1,
                    height: 1
                },
                120.0,
                0.95
            ),
            Err(VerifiedModeError::ModeCoerced)
        );
        assert_eq!(
            revalidate_verified_mode(&descriptor, tuple.resolution, 100.0, 0.95),
            Err(VerifiedModeError::UnderTargetFps)
        );
    }

    fn metrics_for(resolution: Resolution, measured_fps: f64) -> CaptureMetrics {
        CaptureMetrics {
            elapsed: Duration::from_secs(1),
            read_attempts: 2,
            captured_frames: 2,
            empty_frames: 0,
            read_failures: 0,
            measured_fps,
            median_interval: Some(Duration::from_millis(8)),
            p95_interval: Some(Duration::from_millis(8)),
            p99_interval: Some(Duration::from_millis(8)),
            maximum_gap: Some(Duration::from_millis(8)),
            long_gap_count: 0,
            long_gap_ratio: 0.0,
            actual_resolutions: vec![resolution; 2],
        }
    }
}
