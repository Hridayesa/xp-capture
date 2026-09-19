use std::time::{Duration, Instant};

use crate::camera::{
    CaptureAdapterResult, ProbeTarget,
    profiling::{FrameMetadata, ModeTuple, PropertySetDiagnostics, ReportedCaptureProperties},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameRead {
    Empty,
    Frame(FrameMetadata),
}

pub enum CaptureOpen {
    Unavailable,
    Session(Box<dyn CaptureSession>),
}

pub trait CaptureSession: Send {
    fn apply_mode(&mut self, mode: ModeTuple) -> CaptureAdapterResult<PropertySetDiagnostics>;
    fn reported_properties(&mut self) -> CaptureAdapterResult<ReportedCaptureProperties>;
    fn read(&mut self) -> CaptureAdapterResult<FrameRead>;
    fn release(&mut self) -> CaptureAdapterResult<()>;
}

pub trait CaptureAdapter: Send {
    fn open(&mut self, target: ProbeTarget) -> CaptureAdapterResult<CaptureOpen>;
}

pub trait CaptureAdapterFactory: Send + Sync {
    fn create(&self) -> Box<dyn CaptureAdapter>;
}

pub trait MonotonicClock: Send + Sync {
    fn now(&self) -> Duration;
    fn sleep(&self, duration: Duration);
}

#[derive(Debug)]
pub struct SystemMonotonicClock {
    origin: Instant,
}

impl SystemMonotonicClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemMonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MonotonicClock for SystemMonotonicClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}
