use std::{
    fs,
    path::PathBuf,
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use opencv::{
    core::{self, Mat, Scalar, Size, Vec3b, Vector},
    imgcodecs, imgproc,
    prelude::*,
    videoio,
};
use sha2::{Digest, Sha256};

use crate::self_check::{
    AdapterError, OpenCvAdapter, OpenCvSummary, WriterAdapterError, WriterRoundtripSummary,
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
pub struct OpenCvRuntimeAdapter;

impl OpenCvRuntimeAdapter {
    pub fn new() -> Self {
        Self
    }

    fn opencv_error(context: &'static str, source: opencv::Error) -> AdapterError {
        AdapterError::with_source(context, source)
    }

    fn temporary_video() -> TemporaryVideo {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "xp-capture-self-check-{}-{sequence}.avi",
            process::id()
        ));
        TemporaryVideo { path }
    }

    fn perform_writer_roundtrip(
        temporary: &TemporaryVideo,
    ) -> Result<WriterRoundtripSummary, WriterAdapterError> {
        let path = temporary
            .path
            .to_str()
            .ok_or_else(|| WriterAdapterError::Open {
                source: AdapterError::new("temporary video path is not valid UTF-8"),
            })?;
        let size = Size::new(320, 240);
        let fourcc = videoio::VideoWriter::fourcc('M', 'J', 'P', 'G').map_err(|source| {
            WriterAdapterError::Open {
                source: Self::opencv_error("failed to create MJPG fourcc", source),
            }
        })?;
        let mut writer = videoio::VideoWriter::new_with_backend(
            path,
            videoio::CAP_FFMPEG,
            fourcc,
            30.0,
            size,
            true,
        )
        .map_err(|source| WriterAdapterError::Open {
            source: Self::opencv_error("failed to construct CAP_FFMPEG writer", source),
        })?;
        if !writer
            .is_opened()
            .map_err(|source| WriterAdapterError::Open {
                source: Self::opencv_error("failed to query writer open state", source),
            })?
        {
            return Err(WriterAdapterError::Open {
                source: AdapterError::new("CAP_FFMPEG writer did not open"),
            });
        }

        let backend_name =
            writer
                .get_backend_name()
                .map_err(|source| WriterAdapterError::Backend {
                    source: Self::opencv_error("failed to read writer backend", source),
                })?;
        if !backend_name.eq_ignore_ascii_case("FFMPEG") {
            return Err(WriterAdapterError::UnexpectedBackend {
                actual: backend_name,
            });
        }

        for frame_index in 0..30 {
            let color = Scalar::new(
                f64::from((frame_index * 7) % 255),
                f64::from((frame_index * 13) % 255),
                f64::from((frame_index * 19) % 255),
                0.0,
            );
            let frame = Mat::new_rows_cols_with_default(240, 320, Vec3b::opencv_type(), color)
                .map_err(|source| WriterAdapterError::Roundtrip {
                    source: Self::opencv_error("failed to create synthetic frame", source),
                })?;
            writer
                .write(&frame)
                .map_err(|source| WriterAdapterError::Roundtrip {
                    source: Self::opencv_error("failed to write synthetic frame", source),
                })?;
        }
        writer
            .release()
            .map_err(|source| WriterAdapterError::Open {
                source: Self::opencv_error("failed to finalize diagnostic video", source),
            })?;

        let mut capture =
            videoio::VideoCapture::from_file(path, videoio::CAP_FFMPEG).map_err(|source| {
                WriterAdapterError::Roundtrip {
                    source: Self::opencv_error("failed to reopen diagnostic video", source),
                }
            })?;
        if !capture
            .is_opened()
            .map_err(|source| WriterAdapterError::Roundtrip {
                source: Self::opencv_error("failed to query reader open state", source),
            })?
        {
            return Err(WriterAdapterError::Roundtrip {
                source: AdapterError::new("CAP_FFMPEG reader did not open"),
            });
        }

        let mut frame_count = 0_u32;
        loop {
            let mut frame = Mat::default();
            let read =
                capture
                    .read(&mut frame)
                    .map_err(|source| WriterAdapterError::Roundtrip {
                        source: Self::opencv_error("failed to decode diagnostic frame", source),
                    })?;
            if !read || frame.empty() {
                break;
            }
            let decoded_size = frame
                .size()
                .map_err(|source| WriterAdapterError::Roundtrip {
                    source: Self::opencv_error("failed to read decoded frame size", source),
                })?;
            if decoded_size != size {
                return Err(WriterAdapterError::Roundtrip {
                    source: AdapterError::new(format!(
                        "decoded frame has unexpected size {}x{}",
                        decoded_size.width, decoded_size.height
                    )),
                });
            }
            frame_count += 1;
        }
        capture
            .release()
            .map_err(|source| WriterAdapterError::Roundtrip {
                source: Self::opencv_error("failed to close diagnostic reader", source),
            })?;

        Ok(WriterRoundtripSummary {
            backend_name,
            frame_count,
            width: 320,
            height: 240,
        })
    }
}

impl OpenCvAdapter for OpenCvRuntimeAdapter {
    fn load_summary(&self) -> Result<OpenCvSummary, AdapterError> {
        let version = core::get_version_string()
            .map_err(|source| Self::opencv_error("failed to read OpenCV version", source))?;
        let build_information = core::get_build_information().map_err(|source| {
            Self::opencv_error("failed to read OpenCV build information", source)
        })?;
        let build_information_sha256 =
            format!("{:x}", Sha256::digest(build_information.as_bytes()));
        let backend_ids = videoio::get_backends()
            .map_err(|source| Self::opencv_error("failed to enumerate OpenCV backends", source))?;
        let mut available_backends = Vec::with_capacity(backend_ids.len());
        for backend in backend_ids {
            let name = videoio::get_backend_name(backend)
                .map_err(|source| Self::opencv_error("failed to name OpenCV backend", source))?;
            available_backends.push(name);
        }
        available_backends.sort();
        available_backends.dedup();

        Ok(OpenCvSummary {
            version,
            build_information_sha256,
            available_backends,
        })
    }

    fn image_codec_roundtrip(&self) -> Result<(), AdapterError> {
        let source = Mat::new_rows_cols_with_default(
            480,
            640,
            Vec3b::opencv_type(),
            Scalar::new(32.0, 128.0, 224.0, 0.0),
        )
        .map_err(|source| Self::opencv_error("failed to create source image", source))?;
        let mut resized = Mat::default();
        imgproc::resize(
            &source,
            &mut resized,
            Size::new(320, 240),
            0.0,
            0.0,
            imgproc::INTER_AREA,
        )
        .map_err(|source| Self::opencv_error("failed to resize source image", source))?;

        let mut encoded = Vector::<u8>::new();
        let encoded_ok = imgcodecs::imencode_def(".jpg", &resized, &mut encoded)
            .map_err(|source| Self::opencv_error("failed to encode JPEG", source))?;
        if !encoded_ok || encoded.is_empty() {
            return Err(AdapterError::new("OpenCV returned an empty JPEG payload"));
        }
        let decoded = imgcodecs::imdecode(&encoded, imgcodecs::IMREAD_COLOR)
            .map_err(|source| Self::opencv_error("failed to decode JPEG", source))?;
        let decoded_size = decoded
            .size()
            .map_err(|source| Self::opencv_error("failed to read decoded JPEG size", source))?;
        if decoded_size != Size::new(320, 240) {
            return Err(AdapterError::new(format!(
                "decoded JPEG has unexpected size {}x{}",
                decoded_size.width, decoded_size.height
            )));
        }

        Ok(())
    }

    fn writer_roundtrip(&self) -> Result<WriterRoundtripSummary, WriterAdapterError> {
        let temporary = Self::temporary_video();
        let result = Self::perform_writer_roundtrip(&temporary);
        let cleanup = temporary.cleanup();
        match (result, cleanup) {
            (Ok(summary), Ok(())) => Ok(summary),
            (Err(error), _) => Err(error),
            (Ok(_), Err(source)) => Err(WriterAdapterError::Roundtrip {
                source: AdapterError::with_source(
                    "failed to remove temporary diagnostic video",
                    source,
                ),
            }),
        }
    }
}

struct TemporaryVideo {
    path: PathBuf,
}

impl TemporaryVideo {
    fn cleanup(mut self) -> std::io::Result<()> {
        let result = match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        };
        self.path = PathBuf::new();
        result
    }
}

impl Drop for TemporaryVideo {
    fn drop(&mut self) {
        if !self.path.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.path);
        }
    }
}
