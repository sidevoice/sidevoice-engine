//! The audio the loop reads and keeps: WAV files read (the clips) and written (what each model said). Bringing audio
//! to a model's rate is the engine's.

/// The samples of a WAV file (PCM of 16 bits, or IEEE floats of 32), its channels mixed down to one, and their rate.
pub(crate) fn read_wav(wav: &[u8]) -> Result<(Vec<f32>, u32), String> {
    let u16_at = |at: usize| u16::from_le_bytes([wav[at], wav[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes([wav[at], wav[at + 1], wav[at + 2], wav[at + 3]]);
    if wav.len() < 12 || &wav[..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let mut format = None;
    let mut at = 12;
    while at + 8 <= wav.len() {
        let (id, len) = (&wav[at..at + 4], u32_at(at + 4) as usize);
        let body = at + 8;
        let end = body.checked_add(len).filter(|end| *end <= wav.len());
        let Some(end) = end else {
            return Err("a WAV chunk runs past the end".into());
        };
        match id {
            b"fmt " if len >= 16 => {
                // 1: PCM; 3: IEEE float; 0xFFFE: extensible, whose sub-format repeats one of those.
                let tag = match u16_at(body) {
                    0xFFFE if len >= 26 => u16_at(body + 24),
                    tag => tag,
                };
                format = Some((tag, u16_at(body + 2), u32_at(body + 4), u16_at(body + 14)));
            }
            b"data" => {
                let Some((tag, channels, rate, bits)) = format else {
                    return Err("a WAV data chunk before its format".into());
                };
                let samples: Vec<f32> = match (tag, bits) {
                    (1, 16) => wav[body..end]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|sample| f32::from(i16::from_le_bytes(*sample)) / 32_768.0)
                        .collect(),
                    (3, 32) => wav[body..end]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|sample| f32::from_le_bytes(*sample))
                        .collect(),
                    _ => return Err(format!("WAV format {tag} at {bits} bits is not read here")),
                };
                let channels = usize::from(channels.max(1));
                let mono = samples
                    .chunks(channels)
                    .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
                    .collect();
                return Ok((mono, rate));
            }
            _ => {}
        }
        at = end + (len & 1);
    }
    Err("a WAV file with no data".into())
}

/// `samples` as a 16-bit PCM mono WAV file at `rate`.
pub(crate) fn wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let data = u32::try_from(samples.len() * 2).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for sample in samples {
        let pcm = (sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
        out.extend_from_slice(&pcm.to_le_bytes());
    }
    out
}
