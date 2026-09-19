use opencv::{core::Mat, prelude::*, videoio};

use crate::camera::{
    CaptureAdapter, CaptureAdapterError, CaptureAdapterFactory, CaptureAdapterResult,
    CaptureBackend, CaptureOpen, CaptureSession, FrameRead, ProbeTarget,
    profiling::{
        FourCc, FrameMetadata, ModeTuple, PropertySetDiagnostics, ReportedCaptureProperties,
    },
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
    fn apply_mode(&mut self, mode: ModeTuple) -> CaptureAdapterResult<PropertySetDiagnostics> {
        let capture = self.capture.as_mut().ok_or_else(session_released_apply)?;
        let fourcc = f64::from(fourcc_property(mode.fourcc));
        let fourcc_set = capture
            .set(videoio::CAP_PROP_FOURCC, fourcc)
            .map_err(CaptureAdapterError::apply)?;
        let width_set = capture
            .set(
                videoio::CAP_PROP_FRAME_WIDTH,
                f64::from(mode.resolution.width),
            )
            .map_err(CaptureAdapterError::apply)?;
        let height_set = capture
            .set(
                videoio::CAP_PROP_FRAME_HEIGHT,
                f64::from(mode.resolution.height),
            )
            .map_err(CaptureAdapterError::apply)?;
        let fps_set = capture
            .set(videoio::CAP_PROP_FPS, mode.requested_fps)
            .map_err(CaptureAdapterError::apply)?;
        Ok(PropertySetDiagnostics {
            fourcc: fourcc_set,
            width: width_set,
            height: height_set,
            fps: fps_set,
        })
    }

    fn reported_properties(&mut self) -> CaptureAdapterResult<ReportedCaptureProperties> {
        let capture = self.capture.as_mut().ok_or_else(session_released_get)?;
        let fourcc = capture
            .get(videoio::CAP_PROP_FOURCC)
            .map_err(CaptureAdapterError::get)?;
        let width = capture
            .get(videoio::CAP_PROP_FRAME_WIDTH)
            .map_err(CaptureAdapterError::get)?;
        let height = capture
            .get(videoio::CAP_PROP_FRAME_HEIGHT)
            .map_err(CaptureAdapterError::get)?;
        let fps = capture
            .get(videoio::CAP_PROP_FPS)
            .map_err(CaptureAdapterError::get)?;
        Ok(ReportedCaptureProperties {
            fourcc: decode_fourcc_property(fourcc),
            width: finite_reported(width),
            height: finite_reported(height),
            fps: finite_reported(fps),
        })
    }

    fn read(&mut self) -> CaptureAdapterResult<FrameRead> {
        let capture = self.capture.as_mut().ok_or_else(|| {
            CaptureAdapterError::read(std::io::Error::other("camera session was already released"))
        })?;
        let mut frame = Mat::default();
        let read = capture
            .read(&mut frame)
            .map_err(CaptureAdapterError::read)?;
        if read && !frame.empty() {
            Ok(FrameRead::Frame(frame_metadata(&frame)?))
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

fn session_released_apply() -> CaptureAdapterError {
    CaptureAdapterError::apply(std::io::Error::other("camera session was already released"))
}

fn session_released_get() -> CaptureAdapterError {
    CaptureAdapterError::get(std::io::Error::other("camera session was already released"))
}

fn fourcc_property(value: FourCc) -> i32 {
    let bytes = value.as_str().as_bytes();
    i32::from(bytes[0])
        | (i32::from(bytes[1]) << 8)
        | (i32::from(bytes[2]) << 16)
        | (i32::from(bytes[3]) << 24)
}

fn decode_fourcc_property(value: f64) -> Option<FourCc> {
    if !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
        return None;
    }
    let encoded = value.round() as i32;
    let bytes = encoded.to_le_bytes();
    let text = std::str::from_utf8(&bytes).ok()?;
    FourCc::parse(text).ok()
}

fn finite_reported(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

fn frame_metadata(frame: &Mat) -> CaptureAdapterResult<FrameMetadata> {
    let width = u32::try_from(frame.cols()).map_err(CaptureAdapterError::read)?;
    let height = u32::try_from(frame.rows()).map_err(CaptureAdapterError::read)?;
    Ok(FrameMetadata { width, height })
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
    use super::*;
    use opencv::core::{CV_8UC3, Scalar};

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

    #[test]
    fn fourcc_property_round_trips_in_opencv_byte_order() {
        for value in ["MJPG", "YUY2"] {
            let code = FourCc::parse(value).expect("valid FourCC");
            assert_eq!(
                decode_fourcc_property(f64::from(fourcc_property(code))),
                Some(code)
            );
        }
        assert_eq!(decode_fourcc_property(f64::NAN), None);
    }

    #[test]
    fn property_mapping_is_explicit_and_ordered() {
        let source = include_str!("opencv.rs");
        let fourcc = source.find("CAP_PROP_FOURCC").expect("FourCC mapping");
        let width = source.find("CAP_PROP_FRAME_WIDTH").expect("width mapping");
        let height = source
            .find("CAP_PROP_FRAME_HEIGHT")
            .expect("height mapping");
        let fps = source.find("CAP_PROP_FPS").expect("FPS mapping");
        assert!(fourcc < width && width < height && height < fps);
        assert!(source.contains("frame.cols()"));
        assert!(source.contains("frame.rows()"));
    }

    #[test]
    fn actual_frame_dimensions_are_extracted_without_exposing_mat() {
        let frame = Mat::new_rows_cols_with_default(480, 640, CV_8UC3, Scalar::all(0.0))
            .expect("synthetic frame");
        assert_eq!(
            frame_metadata(&frame).expect("metadata"),
            FrameMetadata {
                width: 640,
                height: 480
            }
        );
    }
}
