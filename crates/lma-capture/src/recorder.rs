use std::{
    fs::File,
    io::{self, BufWriter},
    path::Path,
};

pub struct WavRecorder {
    writer: Option<hound::WavWriter<BufWriter<File>>>,
}

impl WavRecorder {
    pub fn create(path: impl AsRef<Path>, sample_rate: u32) -> Result<Self, hound::Error> {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let writer = hound::WavWriter::create(path, spec)?;
        Ok(Self {
            writer: Some(writer),
        })
    }

    pub fn write(&mut self, samples: &[i16]) -> Result<(), hound::Error> {
        let writer = self.writer.as_mut().ok_or_else(|| {
            hound::Error::IoError(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "WAV recorder has already finished",
            ))
        })?;
        for sample in samples {
            writer.write_sample(*sample)?;
        }
        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), hound::Error> {
        if let Some(writer) = self.writer.take() {
            writer.finalize()?;
        }
        Ok(())
    }
}

impl Drop for WavRecorder {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::WavRecorder;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_wav_path() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("lma-capture-{unique}.wav"))
    }

    #[test]
    fn writes_16_bit_stereo_wav_with_the_requested_sample_rate() {
        let path = temporary_wav_path();
        let mut recorder = WavRecorder::create(&path, 48_000).unwrap();
        recorder.write(&[32_767, -32_768, 1, -1]).unwrap();
        recorder.finish().unwrap();

        let mut reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 2);
        assert_eq!(spec.sample_rate, 48_000);
        assert_eq!(spec.bits_per_sample, 16);
        assert_eq!(
            reader
                .samples::<i16>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            [32_767, -32_768, 1, -1]
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn reports_create_errors_for_a_missing_parent_directory() {
        let path = std::env::temp_dir()
            .join("lma-capture-missing-parent")
            .join("recording.wav");
        assert!(WavRecorder::create(path, 48_000).is_err());
    }
}
