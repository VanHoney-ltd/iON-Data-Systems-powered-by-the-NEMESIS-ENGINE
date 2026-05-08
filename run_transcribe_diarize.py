#!/usr/bin/env python3
import os
import sys
import json

AUDIO_PATH = "/home/ghost/iON/cases/pcr/CodyRedmond_20250011928_BWL7044188-0.wav"
OUTPUT_DIR = "/home/ghost/iON/cases/pcr"
WHISPER_MODEL = "small"  # Better accuracy, slower on CPU

def main():
    os.makedirs(OUTPUT_DIR, exist_ok=True)
    base_name = os.path.splitext(os.path.basename(AUDIO_PATH))[0]

    # 1. Transcribe
    print("[1/3] Loading Whisper model...", flush=True)
    import whisper
    model = whisper.load_model(WHISPER_MODEL)
    print(f"[1/3] Transcribing {AUDIO_PATH} ...", flush=True)
    result = model.transcribe(AUDIO_PATH, verbose=False)

    transcript_path = os.path.join(OUTPUT_DIR, f"{base_name}_transcript.json")
    with open(transcript_path, "w") as f:
        json.dump(result, f, indent=2)
    print(f"[1/3] Transcript saved to {transcript_path}", flush=True)

    # 2. Diarize
    print("[2/3] Loading pyannote diarization pipeline...", flush=True)
    from pyannote.audio import Pipeline
    pipeline = Pipeline.from_pretrained(
        "pyannote/speaker-diarization-3.1"
    )
    print(f"[2/3] Running diarization...", flush=True)
    diarization = pipeline(AUDIO_PATH)

    diarization_path = os.path.join(OUTPUT_DIR, f"{base_name}_diarization.txt")
    with open(diarization_path, "w") as f:
        for turn, _, speaker in diarization.itertracks(yield_label=True):
            f.write(f"{speaker} {turn.start:.3f} {turn.end:.3f}\n")
    print(f"[2/3] Diarization saved to {diarization_path}", flush=True)

    # 3. Merge transcripts to speakers
    print("[3/3] Merging transcript with speakers...", flush=True)

    def speaker_at(t):
        for turn, _, spk in diarization.itertracks(yield_label=True):
            if turn.start <= t <= turn.end:
                return spk
        return "UNKNOWN"

    merged_lines = []
    for seg in result["segments"]:
        t = (seg["start"] + seg["end"]) / 2
        spk = speaker_at(t)
        line = f"[{seg['start']:>7.2f}s - {seg['end']:>7.2f}s] {spk}: {seg['text'].strip()}"
        merged_lines.append(line)

    merged_path = os.path.join(OUTPUT_DIR, f"{base_name}_merged.txt")
    with open(merged_path, "w") as f:
        f.write("\n".join(merged_lines))
    print(f"[3/3] Merged transcript saved to {merged_path}", flush=True)
    print("Done.", flush=True)

if __name__ == "__main__":
    main()
