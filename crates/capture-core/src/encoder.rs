//! Synchronous Media Foundation sink writer. WriteSample retains platform
//! throttling, applying backpressure instead of an unbounded application queue.
use anyhow::{bail, Result};
use std::{path::Path, ptr::null_mut};
use windows::{
    core::{GUID, PCWSTR},
    Win32::Media::MediaFoundation::*,
};

struct MediaFoundation;
impl MediaFoundation {
    fn start() -> Result<Self> {
        unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        }
        Ok(Self)
    }
}
impl Drop for MediaFoundation {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
        }
    }
}

pub(crate) struct Encoder {
    // Release the writer before shutting down Media Foundation.
    writer: IMFSinkWriter,
    _foundation: MediaFoundation,
    video_stream: u32,
    audio_stream: Option<u32>,
    frame_duration: i64,
    audio_rate: u32,
    audio_channels: u32,
    audio_frames: u64,
}
impl Encoder {
    pub fn new(
        path: &Path,
        width: u32,
        height: u32,
        fps: u32,
        audio: Option<(u32, u32)>,
    ) -> Result<Self> {
        let foundation = MediaFoundation::start()?;
        unsafe {
            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 2)?;
            let attributes = attributes.expect("MFCreateAttributes succeeded");
            attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            // Do not set MF_SINK_WRITER_DISABLE_THROTTLING: WriteSample must
            // throttle producers when Windows cannot encode at the requested rate.
            let path: Vec<u16> = path
                .as_os_str()
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let writer = MFCreateSinkWriterFromURL(PCWSTR(path.as_ptr()), None, &attributes)?;
            let output = video_type(MFVideoFormat_H264, width, height, fps)?;
            output.SetUINT32(&MF_MT_AVG_BITRATE, 12_000_000)?;
            let video_stream = writer.AddStream(&output)?;
            let input = video_type(MFVideoFormat_RGB32, width, height, fps)?;
            // GDI DIB frames are BGRA, bottom-up; a negative stride describes that orientation.
            input.SetUINT32(&MF_MT_DEFAULT_STRIDE, (-(width as i32 * 4)) as u32)?;
            writer.SetInputMediaType(video_stream, &input, None)?;
            let audio_stream = if let Some((rate, channels)) = audio {
                let output = audio_type(MFAudioFormat_AAC, rate, channels)?;
                output.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24_000)?;
                output.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                output.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0)?;
                output.SetUINT32(&MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, 0x29)?;
                let stream = writer.AddStream(&output)?;
                let input = audio_type(MFAudioFormat_PCM, rate, channels)?;
                input.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                input.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, channels * 2)?;
                input.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * channels * 2)?;
                writer.SetInputMediaType(stream, &input, None)?;
                Some(stream)
            } else {
                None
            };
            writer.BeginWriting()?;
            Ok(Self {
                writer,
                _foundation: foundation,
                video_stream,
                audio_stream,
                frame_duration: 10_000_000 / i64::from(fps),
                audio_rate: audio.map_or(0, |(rate, _)| rate),
                audio_channels: audio.map_or(0, |(_, channels)| channels),
                audio_frames: 0,
            })
        }
    }
    pub fn send_frame_buffer(&mut self, pixels: &[u8], timestamp: i64) -> Result<()> {
        let sample = sample(pixels, timestamp, self.frame_duration)?;
        unsafe {
            self.writer.WriteSample(self.video_stream, &sample)?;
        }
        Ok(())
    }
    pub fn send_audio_buffer(&mut self, pixels: &[u8], _timestamp: i64) -> Result<()> {
        let Some(stream) = self.audio_stream else {
            bail!("Audio is disabled");
        };
        let frames = pixels.len() as u64 / (u64::from(self.audio_channels) * 2);
        if frames == 0 {
            return Ok(());
        }
        let timestamp = self.audio_frames * 10_000_000 / u64::from(self.audio_rate);
        let next = (self.audio_frames + frames) * 10_000_000 / u64::from(self.audio_rate);
        let sample = sample(pixels, timestamp as i64, (next - timestamp) as i64)?;
        unsafe {
            self.writer.WriteSample(stream, &sample)?;
        }
        self.audio_frames += frames;
        Ok(())
    }
    pub fn finish(self) -> Result<()> {
        unsafe {
            self.writer.Finalize()?;
        }
        Ok(())
    }
}
fn video_type(subtype: GUID, width: u32, height: u32, fps: u32) -> Result<IMFMediaType> {
    unsafe {
        let media = MFCreateMediaType()?;
        media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media.SetGUID(&MF_MT_SUBTYPE, &subtype)?;
        media.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        media.SetUINT64(
            &MF_MT_FRAME_SIZE,
            (u64::from(width) << 32) | u64::from(height),
        )?;
        media.SetUINT64(&MF_MT_FRAME_RATE, (u64::from(fps) << 32) | 1)?;
        media.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1)?;
        Ok(media)
    }
}
fn audio_type(subtype: GUID, rate: u32, channels: u32) -> Result<IMFMediaType> {
    unsafe {
        let media = MFCreateMediaType()?;
        media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        media.SetGUID(&MF_MT_SUBTYPE, &subtype)?;
        media.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)?;
        media.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, channels)?;
        Ok(media)
    }
}
fn sample(bytes: &[u8], timestamp: i64, duration: i64) -> Result<IMFSample> {
    unsafe {
        let buffer = MFCreateMemoryBuffer(bytes.len().try_into()?)?;
        let mut destination = null_mut();
        buffer.Lock(&mut destination, None, None)?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
        buffer.Unlock()?;
        buffer.SetCurrentLength(bytes.len() as u32)?;
        let sample = MFCreateSample()?;
        sample.AddBuffer(&buffer)?;
        sample.SetSampleTime(timestamp)?;
        sample.SetSampleDuration(duration)?;
        Ok(sample)
    }
}
