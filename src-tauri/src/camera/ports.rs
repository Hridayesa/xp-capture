use std::time::{Duration, Instant};

use crate::camera::{CaptureAdapterResult, ProbeTarget};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameRead {
    Empty,
    Frame,
}

pub enum CaptureOpen {
    Unavailable,
    Session(Box<dyn CaptureSession>),
}

pub trait CaptureSession: Send {
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
