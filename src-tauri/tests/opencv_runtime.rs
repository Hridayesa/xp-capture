use xp_capture_lib::self_check::{OpenCvAdapter, OpenCvRuntimeAdapter};

#[test]
fn pinned_opencv_runtime_passes_synthetic_roundtrips() {
    if std::env::var_os("XP_CAPTURE_OPENCV_INTEGRATION").is_none() {
        return;
    }

    let adapter = OpenCvRuntimeAdapter::new();
    let summary = adapter.load_summary().expect("OpenCV runtime loads");
    assert_eq!(summary.version, "4.12.0");
    for required in ["MSMF", "DSHOW", "FFMPEG"] {
        assert!(
            summary
                .available_backends
                .iter()
                .any(|actual| actual.eq_ignore_ascii_case(required)),
            "missing backend {required}: {:?}",
            summary.available_backends
        );
    }

    adapter
        .image_codec_roundtrip()
        .expect("JPEG round-trip passes");
    let writer = adapter
        .writer_roundtrip()
        .expect("MJPG/AVI writer round-trip passes");
    assert_eq!(writer.backend_name, "FFMPEG");
    assert_eq!(writer.frame_count, 30);
    assert_eq!((writer.width, writer.height), (320, 240));
}
