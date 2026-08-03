//! Audio/video → transcript markdown via whisper.cpp (`--features transcribe`).
//!
//! Chosen over Apple's SFSpeechRecognizer after research: the Apple path needs
//! an embedded Info.plist (else SIGTRAP), a TCC prompt that unsigned binaries
//! lose on every rebuild, has a documented 1-minute cap, and is unreliable for
//! video containers. whisper has none of that and runs everywhere.
//!
//! Pipeline: symphonia demux+decode (audio track of mp4/mov/mkv/webm too) →
//! mono downmix → linear resample to 16kHz → whisper → sparse-timestamped
//! paragraphs. The model (~574MB, large-v3-turbo q5_0) auto-downloads once to
//! `~/.filegoblin/models/`; override with $FILEGOBLIN_WHISPER_MODEL.

use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::Result;
use std::path::Path;

pub struct TranscribeGobbler;

impl Gobble for TranscribeGobbler {
    #[allow(unused_variables)]
    fn gobble(&self, path: &Path, opts: &ParseOptions) -> Result<String> {
        #[cfg(feature = "transcribe")]
        {
            imp::transcribe_file(path, opts.full, opts.quiet)
        }
        #[cfg(not(feature = "transcribe"))]
        {
            anyhow::bail!(
                "Audio/video transcription requires the 'transcribe' feature. \
                 Recompile with --features transcribe (or transcribe-metal on macOS)."
            )
        }
    }
}

#[cfg(feature = "transcribe")]
mod imp {
    use anyhow::{Context, Result};
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    const MODEL_FILE: &str = "ggml-large-v3-turbo-q5_0.bin";
    const MODEL_URL: &str =
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin";

    use crate::parsers::transcript::Segment;

    pub fn transcribe_file(path: &Path, per_segment: bool, quiet: bool) -> Result<String> {
        // Transcripts are content-addressed. The key must cover everything the
        // *rendered* value depends on, not just the audio: it previously omitted
        // `per_segment` and the model/language overrides, so running again with
        // `--full` served the cached collapsed transcript and the flag silently
        // did nothing, and swapping models returned the old model's text under
        // the new model's name.
        let file_key = format!(
            "{}:{}:{}:per_segment={}",
            hash_file(path)?,
            model_file_name(),
            std::env::var("FILEGOBLIN_WHISPER_LANG").unwrap_or_default(),
            per_segment,
        );
        let cache_key = crate::cache::key(file_key.as_bytes());
        if let Some(cached) = crate::cache::get_blob("transcript", &cache_key) {
            if !quiet {
                eprintln!("♻️  Using cached transcript (--no-cache to redo)");
            }
            return Ok(cached);
        }

        let (pcm, rate) = decode_to_mono_f32(path)?;
        let pcm = resample_linear(&pcm, rate, 16_000);
        let seconds = pcm.len() / 16_000;
        if pcm.is_empty() {
            anyhow::bail!("No audio samples could be decoded from this file");
        }
        if !quiet {
            eprintln!(
                "🎙️ Transcribing ~{}m{:02}s of audio (this can take a while on CPU)...",
                seconds / 60,
                seconds % 60
            );
        }

        let (lang, segments) = run_whisper(&pcm)?;
        let out = format_transcript(path, &lang, &segments, per_segment);
        crate::cache::put_blob("transcript", &cache_key, &out);
        Ok(out)
    }

    /// Streaming sha256 of the media file (they can be GBs).
    fn hash_file(path: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let mut f = std::fs::File::open(path).context("Failed to open media file")?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(format!("{:x}", h.finalize()))
    }

    /// Demuxes and decodes the first audio track to interleaved-downmixed mono
    /// f32 at the source sample rate.
    pub fn decode_to_mono_f32(path: &Path) -> Result<(Vec<f32>, u32)> {
        use symphonia::core::audio::SampleBuffer;
        use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
        use symphonia::core::errors::Error as SymErr;
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;

        let src = std::fs::File::open(path).context("Failed to open media file")?;
        let mss = MediaSourceStream::new(Box::new(src), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .context("Unrecognized media container")?;
        let mut format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| anyhow::anyhow!("No audio track found in this file"))?;
        let track_id = track.id;
        let codec = track.codec_params.codec;

        // Opus (webm/mkv/ogg/caf): symphonia demuxes it but has no decoder, so
        // packets go through libopus. It resamples internally — ask for 16kHz
        // mono-downmixable output directly, exactly what whisper wants.
        if codec == symphonia::core::codecs::CODEC_TYPE_OPUS {
            let ch = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);
            let opus_ch = if ch >= 2 {
                opus::Channels::Stereo
            } else {
                opus::Channels::Mono
            };
            let n_ch = if ch >= 2 { 2 } else { 1 };
            let mut dec = opus::Decoder::new(16_000, opus_ch)
                .map_err(|e| anyhow::anyhow!("libopus init failed: {e}"))?;
            let mut pcm: Vec<f32> = Vec::new();
            // Max opus frame: 120ms @ 16k per channel.
            let mut frame = vec![0f32; 1920 * n_ch];
            loop {
                let packet = match format.next_packet() {
                    Ok(p) => p,
                    Err(SymErr::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        break;
                    }
                    Err(SymErr::ResetRequired) => break,
                    Err(e) => return Err(e).context("Failed while reading media packets"),
                };
                if packet.track_id() != track_id {
                    continue;
                }
                // A corrupt packet mid-stream shouldn't kill the transcript.
                let Ok(samples) = dec.decode_float(&packet.data, &mut frame, false) else {
                    continue;
                };
                for f in frame[..samples * n_ch].chunks(n_ch) {
                    pcm.push(f.iter().sum::<f32>() / n_ch as f32);
                }
            }
            return Ok((pcm, 16_000));
        }

        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|_| {
                anyhow::anyhow!(
                    "Unsupported audio codec ({codec:?}). \
                     Convert first: ffmpeg -i {} -ar 16000 -ac 1 out.wav",
                    path.display()
                )
            })?;

        let mut pcm: Vec<f32> = Vec::new();
        let mut rate = track.codec_params.sample_rate.unwrap_or(16_000);
        loop {
            let packet = match format.next_packet() {
                Ok(p) => p,
                Err(SymErr::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(SymErr::ResetRequired) => break,
                Err(e) => return Err(e).context("Failed while reading media packets"),
            };
            if packet.track_id() != track_id {
                continue;
            }
            match decoder.decode(&packet) {
                Ok(decoded) => {
                    let spec = *decoded.spec();
                    rate = spec.rate;
                    let ch = spec.channels.count().max(1);
                    let mut sbuf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                    sbuf.copy_interleaved_ref(decoded);
                    for frame in sbuf.samples().chunks(ch) {
                        pcm.push(frame.iter().sum::<f32>() / ch as f32);
                    }
                }
                // A corrupt packet mid-stream shouldn't kill the transcript.
                Err(SymErr::DecodeError(_)) => continue,
                Err(_) => break,
            }
        }
        Ok((pcm, rate))
    }

    /// Linear-interpolation resampler. Whisper is robust to resampler quality,
    /// so this beats carrying a DSP dependency.
    pub fn resample_linear(pcm: &[f32], from: u32, to: u32) -> Vec<f32> {
        if from == to || pcm.is_empty() {
            return pcm.to_vec();
        }
        let ratio = from as f64 / to as f64;
        let out_len = (pcm.len() as f64 / ratio) as usize;
        (0..out_len)
            .map(|i| {
                let src = i as f64 * ratio;
                let i0 = (src as usize).min(pcm.len() - 1);
                let i1 = (i0 + 1).min(pcm.len() - 1);
                let frac = (src - i0 as f64) as f32;
                pcm[i0] + (pcm[i1] - pcm[i0]) * frac
            })
            .collect()
    }

    /// Inference is serialized (a horde over a media folder must not
    /// oversubscribe the GPU) and the context is created per call, NOT kept in
    /// a static: statics never drop, and ggml's Metal device asserts at
    /// process exit if a context's residency sets are still alive
    /// (`ggml_metal_device_free: GGML_ASSERT([rsets->data count] == 0)`).
    /// Repeat files are served from the transcript cache anyway.
    fn run_whisper(pcm: &[f32]) -> Result<(String, Vec<Segment>)> {
        use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

        static INFER: Mutex<()> = Mutex::new(());
        static QUIET_LOGS: OnceLock<()> = OnceLock::new();
        // No log backend feature is enabled, so this silences whisper/ggml's
        // (very chatty) stderr output entirely.
        QUIET_LOGS.get_or_init(whisper_rs::install_logging_hooks);

        // `.unwrap()` here poisoned the mutex for the rest of the run: one
        // panicking inference made every *later* file in a horde fail with an
        // unrelated "PoisonError". The lock guards ordering, not shared state,
        // so a poisoned lock is safe to reuse.
        let _serial = INFER.lock().unwrap_or_else(|e| e.into_inner());
        let model = whisper_model_path()?;
        let ctx = WhisperContext::new_with_params(&model, WhisperContextParameters::default())
            .map_err(|e| anyhow::anyhow!("Failed to load whisper model: {e}"))?;
        let mut state = ctx.create_state().context("whisper state")?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let lang_override = std::env::var("FILEGOBLIN_WHISPER_LANG").ok();
        params.set_language(Some(lang_override.as_deref().unwrap_or("auto")));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(4) as i32;
        params.set_n_threads(threads);

        state
            .full(params, pcm)
            .context("whisper inference failed")?;

        let lang = whisper_rs::get_lang_str(state.full_lang_id_from_state())
            .unwrap_or("unknown")
            .to_string();
        let n = state.full_n_segments();
        let mut segments = Vec::with_capacity(n.max(0) as usize);
        for i in 0..n {
            let Some(seg) = state.get_segment(i) else {
                continue;
            };
            let text = seg
                .to_str_lossy()
                .map(|c| c.into_owned())
                .unwrap_or_default();
            if !text.trim().is_empty() {
                segments.push(Segment {
                    start_cs: seg.start_timestamp(),
                    text: text.trim().to_string(),
                });
            }
        }
        Ok((lang, segments))
    }

    /// Identity of the model actually in use, for the transcript cache key.
    /// Hardcoding `MODEL_FILE` meant an override served the other model's
    /// transcripts.
    fn model_file_name() -> String {
        std::env::var("FILEGOBLIN_WHISPER_MODEL").unwrap_or_else(|_| MODEL_FILE.to_string())
    }

    /// $FILEGOBLIN_WHISPER_MODEL → ~/.filegoblin/models/<default> → download.
    fn whisper_model_path() -> Result<PathBuf> {
        if let Ok(p) = std::env::var("FILEGOBLIN_WHISPER_MODEL") {
            let p = PathBuf::from(p);
            if p.exists() {
                return Ok(p);
            }
            anyhow::bail!(
                "$FILEGOBLIN_WHISPER_MODEL points at a missing file: {}",
                p.display()
            );
        }
        let dir = home::home_dir()
            .ok_or_else(|| anyhow::anyhow!("No home directory"))?
            .join(".filegoblin")
            .join("models");
        let model = dir.join(MODEL_FILE);
        if model.exists() {
            return Ok(model);
        }
        download_model(&model)?;
        Ok(model)
    }

    /// Minimum plausible size of the model file. A download that ends early on
    /// a response with no `content-length` (chunked, or a dropped connection)
    /// otherwise installed a truncated file that then failed to load with an
    /// opaque whisper error on *every* future run until deleted by hand.
    const MIN_MODEL_BYTES: u64 = 100 * 1024 * 1024;

    /// One-time model download with atomic install (.part + rename).
    fn download_model(dest: &Path) -> Result<()> {
        use std::io::{Read, Write};
        eprintln!(
            "🧠 Downloading the whisper model (~574 MB, one time) to {} ...",
            dest.display()
        );
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).context("Failed to create models dir")?;
        }
        let resp = ureq::builder()
            // Without a timeout a stalled connection hangs the process forever.
            .timeout_connect(std::time::Duration::from_secs(30))
            .build()
            .get(MODEL_URL)
            .call()
            .with_context(|| {
                format!(
                    "Model download failed. Manual path: curl -L {} -o {}",
                    MODEL_URL,
                    dest.display()
                )
            })?;
        let total = resp
            .header("content-length")
            .and_then(|s| s.parse::<u64>().ok());
        let mut reader = resp.into_reader();
        let part = dest.with_extension("part");
        let mut out = std::fs::File::create(&part).context("Failed to create model file")?;
        let mut buf = vec![0u8; 1 << 20];
        let mut done: u64 = 0;
        let mut last_note = 0u64;
        loop {
            // A read error mid-download must not leave a half file behind.
            let n = match reader.read(&mut buf) {
                Ok(n) => n,
                Err(e) => {
                    drop(out);
                    let _ = std::fs::remove_file(&part);
                    return Err(anyhow::anyhow!("Model download failed mid-transfer: {}", e));
                }
            };
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            done += n as u64;
            if done - last_note > 50 * (1 << 20) {
                last_note = done;
                match total {
                    Some(t) => eprintln!("   … {} / {} MB", done >> 20, t >> 20),
                    None => eprintln!("   … {} MB", done >> 20),
                }
            }
        }
        drop(out);
        if let Some(t) = total
            && done != t
        {
            let _ = std::fs::remove_file(&part);
            anyhow::bail!("Model download truncated ({done} of {t} bytes)");
        }
        // No declared length (chunked response): fall back to a sanity floor
        // rather than installing whatever arrived.
        if total.is_none() && done < MIN_MODEL_BYTES {
            let _ = std::fs::remove_file(&part);
            anyhow::bail!(
                "Model download looks truncated ({} MB received, expected ~574 MB). \
                 Manual path: curl -L {} -o {}",
                done >> 20,
                MODEL_URL,
                dest.display()
            );
        }
        std::fs::rename(&part, dest).context("Failed to finalize model file")?;
        eprintln!("   Brains installed.");
        Ok(())
    }

    /// Delegates to the shared transcript renderer so a whisper transcript and
    /// a subtitle file produce the same markdown shape.
    pub fn format_transcript(
        path: &Path,
        lang: &str,
        segments: &[Segment],
        per_segment: bool,
    ) -> String {
        let meta = vec![
            ("Model", "whisper large-v3-turbo".to_string()),
            ("Language", lang.to_string()),
        ];
        crate::parsers::transcript::render(
            &path.file_name().unwrap_or_default().to_string_lossy(),
            &meta,
            segments,
            per_segment,
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn resample_halves_and_identity() {
            let pcm: Vec<f32> = (0..1000).map(|i| (i as f32).sin()).collect();
            assert_eq!(resample_linear(&pcm, 16_000, 16_000).len(), 1000);
            let down = resample_linear(&pcm, 32_000, 16_000);
            assert!((499..=501).contains(&down.len()), "got {}", down.len());
        }

        #[test]
        fn transcript_formatting_sparse_and_full() {
            let segs = vec![
                Segment {
                    start_cs: 0,
                    text: "Hello there.".into(),
                },
                Segment {
                    start_cs: 150,
                    text: "Second sentence.".into(),
                },
                // 5s silence gap → new paragraph
                Segment {
                    start_cs: 700,
                    text: "After a pause.".into(),
                },
            ];
            let p = Path::new("meeting.m4a");
            let sparse = format_transcript(p, "en", &segs, false);
            assert!(sparse.contains("# Transcript: meeting.m4a"));
            assert!(sparse.contains("**[00:00:00]** Hello there. Second sentence."));
            assert!(sparse.contains("**[00:00:07]** After a pause."));
            let full = format_transcript(p, "en", &segs, true);
            assert!(full.contains("**[00:00:01]** Second sentence."));
        }

        /// Decodes a real spoken-word wav fixture (generated with macOS `say`).
        #[test]
        fn decodes_wav_fixture() {
            let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
            let p = Path::new(&manifest).join("../../test_assets/spoken_sample.wav");
            let (pcm, rate) = decode_to_mono_f32(&p).unwrap();
            assert!(rate >= 8000);
            assert!(pcm.len() as u32 > rate, "expect >1s of audio");
        }

        /// Opus-in-CAF decodes through libopus (regression for the webm gap).
        #[test]
        fn decodes_opus_fixture() {
            let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
            let p = Path::new(&manifest).join("../../test_assets/spoken_opus.caf");
            let (pcm, rate) = decode_to_mono_f32(&p).unwrap();
            assert_eq!(rate, 16_000);
            assert!(pcm.len() as u32 > rate, "expect >1s of audio");
        }

        /// Full pipeline against the real model — skips unless the model is
        /// already installed (574MB download shouldn't run in every CI).
        #[test]
        fn whisper_end_to_end_when_model_present() {
            let model_present = std::env::var("FILEGOBLIN_WHISPER_MODEL")
                .map(|p| Path::new(&p).exists())
                .unwrap_or_else(|_| {
                    home::home_dir()
                        .map(|h| h.join(".filegoblin/models").join(MODEL_FILE).exists())
                        .unwrap_or(false)
                });
            if !model_present {
                // A silent `return` reports "ok" and looks like coverage.
                eprintln!(
                    "SKIPPING the whisper end-to-end assertions: model not installed \
                     (`just get-whisper`, or set $FILEGOBLIN_WHISPER_MODEL)"
                );
                return;
            }
            let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
            let p = Path::new(&manifest).join("../../test_assets/spoken_sample.wav");
            let out = transcribe_file(&p, false, true).unwrap();
            let lower = out.to_lowercase();
            assert!(
                lower.contains("goblin"),
                "transcript should hear 'goblin': {out}"
            );
        }
    }
}
