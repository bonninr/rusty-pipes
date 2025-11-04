use anyhow::{anyhow, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use hound::{WavSpec, WavWriter};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// A simple struct to hold the format info we care about.
#[derive(Debug, Clone, Copy)]
struct WavFormat {
    audio_format: u16,
    channel_count: u16,
    sampling_rate: u32,
    bits_per_sample: u16,
}

/// Checks a .wav file. If it's 24-bit, converts it to a 16-bit copy
/// and returns the *relative path* to the new file.
/// If it's 16-bit, returns the original *relative path*.
/// Skips conversion if the 16-bit version already exists.
pub fn convert_to_16bit_if_needed(relative_path: &Path, base_dir: &Path) -> Result<PathBuf> {
    let full_path = base_dir.join(relative_path);
    if !full_path.exists() {
        return Err(anyhow!("Sample file not found: {:?}", full_path));
    }

    // Create the new path, e.g., ".../sample.wav" -> ".../sample.16.wav"
    let new_extension = match relative_path.extension() {
        Some(ext) => format!("{}.16.wav", ext.to_str().unwrap_or("wav")),
        None => "16.wav".to_string(),
    };
    let new_relative_path = relative_path.with_extension(new_extension);
    let new_full_path = base_dir.join(&new_relative_path);

    // 1. Skip if 16-bit version already exists
    if new_full_path.exists() {
        return Ok(new_relative_path);
    }

    // 2. Manually parse the file
    let mut file = File::open(&full_path)
        .map_err(|e| anyhow!("Manual RIFF: Failed to open {:?}: {}", full_path, e))?;
    let mut reader = BufReader::new(file);

    // Check RIFF header
    let mut riff_header = [0; 4];
    reader.read_exact(&mut riff_header)?;
    if &riff_header != b"RIFF" {
        return Err(anyhow!("Not a RIFF file: {:?}", full_path));
    }

    let _file_size = reader.read_u32::<LittleEndian>()?;

    let mut wave_header = [0; 4];
    reader.read_exact(&mut wave_header)?;
    if &wave_header != b"WAVE" {
        return Err(anyhow!("Not a WAVE file: {:?}", full_path));
    }

    // --- Loop through chunks ---
    let mut format_chunk: Option<WavFormat> = None;
    let mut data_chunk_offset: Option<u64> = None;
    let mut data_chunk_size: Option<u32> = None;

    while let Ok(mut chunk_id) = reader.read_u32::<LittleEndian>().map(|id| id.to_le_bytes()) {
        let chunk_size = reader.read_u32::<LittleEndian>()?;
        let next_chunk_pos = reader.stream_position()? + chunk_size as u64;
        
        // Ensure chunk size is even for correct padding/alignment
        let aligned_chunk_size = chunk_size + (chunk_size % 2);

        match &chunk_id {
            b"fmt " => {
                let audio_format = reader.read_u16::<LittleEndian>()?;
                let channel_count = reader.read_u16::<LittleEndian>()?;
                let sampling_rate = reader.read_u32::<LittleEndian>()?;
                let _byte_rate = reader.read_u32::<LittleEndian>()?;
                let _block_align = reader.read_u16::<LittleEndian>()?;
                let bits_per_sample = reader.read_u16::<LittleEndian>()?;

                format_chunk = Some(WavFormat {
                    audio_format,
                    channel_count,
                    sampling_rate,
                    bits_per_sample,
                });

                // Read any extra format bytes to stay aligned
                let bytes_read = 16;
                if chunk_size > bytes_read {
                    reader.seek(SeekFrom::Current(chunk_size as i64 - bytes_read as i64))?;
                }
            }
            b"data" => {
                // We found the data chunk. Stop looking.
                data_chunk_offset = Some(reader.stream_position()?);
                data_chunk_size = Some(chunk_size);
                break; // Found what we need
            }
            _ => {
                // Unknown chunk (like `smpl`), just skip it
                reader.seek(SeekFrom::Start(next_chunk_pos))?;
            }
        }
        
        // Handle alignment padding
        if next_chunk_pos % 2 != 0 {
             reader.seek(SeekFrom::Start(next_chunk_pos + 1))?;
        } else {
             reader.seek(SeekFrom::Start(next_chunk_pos))?;
        }
    }

    // --- 3. Process the results ---
    let format = format_chunk.ok_or_else(|| anyhow!("File has no 'fmt ' chunk: {:?}", full_path))?;
    let data_offset =
        data_chunk_offset.ok_or_else(|| anyhow!("File has no 'data' chunk: {:?}", full_path))?;
    let data_size = data_chunk_size.unwrap();

    match format.bits_per_sample {
        16 => {
            // It's already 16-bit, no conversion needed.
            Ok(relative_path.to_path_buf())
        }
        24 => {
            // --- This is the conversion case ---
            println!(
                "[WavConvert] Converting 24-bit file: {:?}",
                full_path
            );

            // Create new 16-bit header for hound
            let new_spec = WavSpec {
                channels: format.channel_count,
                sample_rate: format.sampling_rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };

            // Create the writer
            let mut writer = WavWriter::create(&new_full_path, new_spec).map_err(|e| {
                anyhow!("hound::WavWriter failed to create {:?}: {}", new_full_path, e)
            })?;

            // Seek back to the data chunk
            reader.seek(SeekFrom::Start(data_offset))?;
            
            // Read 24-bit samples (3 bytes each) and write as 16-bit
            let num_samples = data_size / 3;
            let mut sample_buf = [0; 3];

            for _ in 0..num_samples {
                reader.read_exact(&mut sample_buf)?;
                // Convert 3-byte (24-bit) LE sample to i32
                let sample_i32 =
                    (sample_buf[0] as i32) | ((sample_buf[1] as i32) << 8) | ((sample_buf[2] as i32) << 16);
                
                // Sign-extend from 24-bit to 32-bit
                let sample_i32_signed = (sample_i32 << 8) >> 8;
                
                // Convert to 16-bit (take the high 16 bits)
                let sample_i16 = (sample_i32_signed >> 8) as i16;
                
                writer.write_sample(sample_i16).map_err(|e| {
                    anyhow!("Failed to write 16-bit sample to {:?}: {}", new_full_path, e)
                })?;
            }

            writer.finalize().map_err(|e| {
                anyhow!("Failed to finalize 16-bit wav file {:?}: {}", new_full_path, e)
            })?;

            Ok(new_relative_path)
        }
        _ => Err(anyhow!(
            "Unsupported bits per sample ({}) for file {:?}",
            format.bits_per_sample,
            full_path
        )),
    }
}

