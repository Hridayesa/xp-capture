use opencv::{core::Mat, prelude::*, videoio};

use crate::camera::{
    CaptureAdapter, CaptureAdapterError, CaptureAdapterFactory, CaptureAdapterResult,
    CaptureBackend, CaptureOpen, CaptureSession, FrameRead, ProbeTarget,
};

#[derive(Default)]
pub struct OpenCvCaptureAdapterFactory;

impl OpenCvCaptureAdapterFactory {
    pub fn new() -> Self {
        Self
    }
}

impl CaptureAdapterFactory for OpenCvCaptureAdapterFactory {
    fn create(&self) -> Box<dyn CaptureAdapter> {
        Box::new(OpenCvCaptureAdapter)
    }
}

struct OpenCvCaptureAdapter;

impl CaptureAdapter for OpenCvCaptureAdapter {
    fn open(&mut self, target: ProbeTarget) -> CaptureAdapterResult<CaptureOpen> {
        let api_preference = match target.backend() {
            CaptureBackend::Msmf => videoio::CAP_MSMF,
            CaptureBackend::Dshow => videoio::CAP_DSHOW,
        };
        let numeric_index = i32::try_from(target.numeric_index()).map_err(|source| {
            CaptureAdapterError::open(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                source,
            ))
        })?;
        let mut capture = videoio::VideoCapture::new(numeric_index, api_preference)
            .map_err(CaptureAdapterError::open)?;
        let opened = match capture.is_opened() {
            Ok(opened) => opened,
            Err(source) => {
                capture.release().map_err(CaptureAdapterError::release)?;
                return Err(CaptureAdapterError::open_state(source));
            }
        };
        if !opened {
            capture.release().map_err(CaptureAdapterError::release)?;
            return Ok(CaptureOpen::Unavailable);
        }
        Ok(CaptureOpen::Session(Box::new(OpenCvCaptureSession {
            capture: Some(capture),
        })))
    }
}

struct OpenCvCaptureSession {
    capture: Option<videoio::VideoCapture>,
}

impl CaptureSession for OpenCvCaptureSession {
    fn read(&mut self) -> CaptureAdapterResult<FrameRead> {
        let capture = self.capture.as_mut().ok_or_else(|| {
            CaptureAdapterError::read(std::io::Error::other("camera session was already released"))
        })?;
        let mut frame = Mat::default();
        let read = capture
            .read(&mut frame)
            .map_err(CaptureAdapterError::read)?;
        if read && !frame.empty() {
            Ok(FrameRead::Frame)
        } else {
            Ok(FrameRead::Empty)
        }
    }

    fn release(&mut self) -> CaptureAdapterResult<()> {
        let Some(mut capture) = self.capture.take() else {
            return Ok(());
        };
        capture.release().map_err(CaptureAdapterError::release)
    }
}

impl Drop for OpenCvCaptureSession {
    fn drop(&mut self) {
        if let Some(capture) = self.capture.as_mut() {
            let _ = capture.release();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn adapter_uses_explicit_windows_backends_and_scoped_frames() {
        let source = include_str!("opencv.rs");
        assert!(source.contains("videoio::CAP_MSMF"));
        assert!(source.contains("videoio::CAP_DSHOW"));
        assert!(!source.contains(&["videoio::CAP_", "ANY"].concat()));
        assert!(source.contains("VideoCapture::new"));
        assert!(source.contains("capture.release()"));
        assert!(!source.contains(&["pub struct OpenCv", "CaptureSession"].concat()));
    }
}
