use anyhow::{anyhow, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// A simple struct to hold the format info we care about.
#[derive(Debug, Clone, Copy)]
struct WavFormat {
    audio_format: u16,
    channel_count: u16,
    sampling_rate: u32,
    bits_per_sample: u16,
}

/// A struct to hold metadata chunks (like 'smpl') that we want to preserve.
#[derive(Debug, Clone)]
struct OtherChunk {
    id: [u8; 4],
    data: Vec<u8>,
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
    let file = File::open(&full_path)
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

    // --- Loop through all chunks ---
    let mut format_chunk: Option<WavFormat> = None;
    let mut data_chunk_info: Option<(u64, u32)> = None; // (offset, size)
    let mut other_chunks: Vec<OtherChunk> = Vec::new();

    while let Ok(chunk_id) = reader.read_u32::<LittleEndian>().map(|id| id.to_le_bytes()) {
        let chunk_size = reader.read_u32::<LittleEndian>()?;
        let chunk_data_start_pos = reader.stream_position()?;
        // Calculate the start of the next chunk, including padding
        let next_chunk_aligned_pos =
            chunk_data_start_pos + (chunk_size as u64 + (chunk_size % 2) as u64);

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
            }
            b"data" => {
                // We found the data chunk. Record its position and size.
                // We will skip reading the data for now.
                data_chunk_info = Some((chunk_data_start_pos, chunk_size));
            }
            _ => {
                // Unknown or metadata chunk (like `smpl`), read and store it
                let mut chunk_data = vec![0; chunk_size as usize];
                reader.read_exact(&mut chunk_data)?;
                other_chunks.push(OtherChunk {
                    id: chunk_id,
                    data: chunk_data,
                });
            }
        }

        // Seek to the start of the next chunk.
        // This robustly handles:
        // 1. Partially read chunks (like `fmt `)
        // 2. Fully read chunks (like `_`)
        // 3. Unread chunks (like `data`)
        // 4. Padding bytes
        reader.seek(SeekFrom::Start(next_chunk_aligned_pos))?;
    }

    // --- 3. Process the results ---
    let format =
        format_chunk.ok_or_else(|| anyhow!("File has no 'fmt ' chunk: {:?}", full_path))?;
    let (data_offset, data_size) =
        data_chunk_info.ok_or_else(|| anyhow!("File has no 'data' chunk: {:?}", full_path))?;

    match format.bits_per_sample {
        16 => {
            // It's already 16-bit, no conversion needed.
            Ok(relative_path.to_path_buf())
        }
        24 => {
            // --- This is the conversion case ---
            println!(
                "[WavConvert] Converting 24-bit file (preserving metadata): {:?}",
                full_path
            );

            // 1. Calculate new 16-bit format specs
            let new_bits_per_sample: u16 = 16;
            let new_block_align = format.channel_count * (new_bits_per_sample / 8);
            let new_byte_rate = format.sampling_rate * new_block_align as u32;

            // 2. Calculate new data chunk size
            // Original data size is in bytes. num 24-bit samples = data_size / 3.
            // New data size = num samples * 2 bytes/sample.
            let num_samples = data_size / 3;
            let new_data_size = num_samples * 2; // 2 bytes per 16-bit sample

            // 3. Calculate total file size for the new RIFF header
            let mut other_chunks_total_size: u32 = 0;
            for chunk in &other_chunks {
                other_chunks_total_size += 8; // (id + size)
                let data_len = chunk.data.len() as u32;
                other_chunks_total_size += data_len + (data_len % 2); // data + padding
            }

            // File size = "WAVE" (4)
            // + "fmt " chunk (8 + 16)
            // + all other chunks (other_chunks_total_size)
            // + "data" chunk (8 + new_data_size)
            let new_riff_file_size =
                4 + (8 + 16) + other_chunks_total_size + (8 + new_data_size);

            // 4. Open writer
            let out_file = File::create(&new_full_path)
                .map_err(|e| anyhow!("Failed to create new file {:?}: {}", new_full_path, e))?;
            let mut writer = BufWriter::new(out_file);

            // 5. Write headers
            writer.write_all(b"RIFF")?;
            writer.write_u32::<LittleEndian>(new_riff_file_size)?;
            writer.write_all(b"WAVE")?;

            // 6. Write "fmt " chunk (16-bit version)
            writer.write_all(b"fmt ")?;
            writer.write_u32::<LittleEndian>(16)?; // chunk size (minimal PCM)
            writer.write_u16::<LittleEndian>(format.audio_format)?; // 1 = PCM
            writer.write_u16::<LittleEndian>(format.channel_count)?;
            writer.write_u32::<LittleEndian>(format.sampling_rate)?;
            writer.write_u32::<LittleEndian>(new_byte_rate)?;
            writer.write_u16::<LittleEndian>(new_block_align)?;
            writer.write_u16::<LittleEndian>(new_bits_per_sample)?;

            // 7. Write all OTHER chunks (e.g., "smpl")
            for chunk in &other_chunks {
                writer.write_all(&chunk.id)?;
                writer.write_u32::<LittleEndian>(chunk.data.len() as u32)?;
                writer.write_all(&chunk.data)?;
                if chunk.data.len() % 2 != 0 {
                    writer.write_u8(0)?; // padding byte
                }
            }

            // 8. Write "data" chunk header
            writer.write_all(b"data")?;
            writer.write_u32::<LittleEndian>(new_data_size)?;

            // 9. Get original file reader and seek to data
            let mut reader = reader.into_inner(); // Get back the File
            reader.seek(SeekFrom::Start(data_offset))?;
            let mut data_reader = BufReader::new(reader);

            // 10. Read 24-bit, convert, write 16-bit
            let mut sample_buf = [0; 3];

            for _ in 0..num_samples {
                data_reader.read_exact(&mut sample_buf)?;
                // Convert 3-byte (24-bit) LE sample to i32
                let sample_i32 = (sample_buf[0] as i32)
                    | ((sample_buf[1] as i32) << 8)
                    | ((sample_buf[2] as i32) << 16);

                // Sign-extend from 24-bit to 32-bit
                let sample_i32_signed = (sample_i32 << 8) >> 8;

                // Convert to 16-bit (dither by truncation, just take high 16 bits)
                let sample_i16 = (sample_i32_signed >> 8) as i16;

                writer.write_i16::<LittleEndian>(sample_i16)?;
            }

            // 11. Finalize
            writer.flush().map_err(|e| {
                anyhow!(
                    "Failed to flush writer for {:?}: {}",
                    new_full_path,
                    e
                )
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