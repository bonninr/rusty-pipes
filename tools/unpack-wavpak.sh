#!/bin/bash

# This script decodes all .wav files in wavpak format and replaces them with regular PCM wav versions.

# Find all .wav files recursively
find . -type f -name "*.wav" | while read -r file; do
    # Check if the file is actually WavPack encoded
    if ffprobe -v error -show_entries stream=codec_name -of default=noprint_wrappers=1:nokey=1 "$file" | grep -q "wavpack"; then
        echo "Converting: $file"
        
        # Define a temporary filename
        temp_file="${file%.wav}.tmp.wav"
        
        # Convert to PCM (16-bit or 24-bit depending on source)
        if ffmpeg -i "$file" -c:a pcm_s16le "$temp_file" -y -loglevel error; then
            # Replace the original with the new PCM version
            mv "$temp_file" "$file"
            echo "Successfully converted $file to PCM."
        else
            echo "Error converting $file. Keeping original."
            rm -f "$temp_file"
        fi
    else
        echo "Skipping: $file (Already PCM or not WavPack)"
    fi
done

echo "Done!"
