#!/usr/bin/env python3
"""
Hermes Transcription Helper
Called by the Hermes Rust agent for each audio/video file requiring
transcription and speaker diarization.

Usage:
    python hermes_transcribe.py <input_media> <output_dir> <base_name>
"""
import sys
import os
import json
import subprocess
import whisper
from pyannote.audio import Pipeline


def extract_audio(input_path: str, output_dir: str, base_name: str) -> str:
    """Convert any media to 16kHz mono WAV for Whisper + pyannote."""
    wav_path = os.path.join(output_dir, f"{base_name}_audio.wav")
    cmd = [
        "ffmpeg", "-y", "-i", input_path,
        "-vn", "-acodec", "pcm_s16le",
        "-ar", "16000", "-ac", "1",
        wav_path
    ]
    subprocess.run(cmd, check=True, capture_output=True)
    return wav_path


def transcribe(wav_path: str, base_name: str) -> dict:
    """Run Whisper small model."""
    model = whisper.load_model("small")
    result = model.transcribe(wav_path, verbose=False)
    return result


def diarize(wav_path: str) -> dict:
    """Run pyannote speaker diarization."""
    pipeline = Pipeline.from_pretrained("pyannote/speaker-diarization-3.1")
    output = pipeline(wav_path)
    return output


def merge(transcript: dict, diarization_output, base_name: str) -> str:
    """Merge Whisper segments with speaker labels."""
    diarization = diarization_output.speaker_diarization

    def speaker_at(t: float) -> str:
        for turn, _, spk in diarization.itertracks(yield_label=True):
            if turn.start <= t <= turn.end:
                return spk
        return "UNKNOWN"

    lines = []
    for seg in transcript.get("segments", []):
        t = (seg["start"] + seg["end"]) / 2
        spk = speaker_at(t)
        line = f"[{seg['start']:>7.2f}s - {seg['end']:>7.2f}s] {spk}: {seg['text'].strip()}"
        lines.append(line)

    return "\n".join(lines)


def generate_pdf(merged_text: str, output_dir: str, base_name: str, source_path: str) -> str:
    """Generate a styled HTML report and convert to PDF."""
    html_path = os.path.join(output_dir, f"{base_name}_report.html")
    pdf_path = os.path.join(output_dir, f"{base_name}_report.pdf")

    lines_html = ""
    for line in merged_text.split("\n"):
        lines_html += f'<div style="margin-bottom:6px;font-family:monospace;font-size:10pt;">{line}</div>\n'

    html = f"""<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<style>
body {{ font-family: "Segoe UI", Roboto, sans-serif; font-size: 10pt; color: #1a1a1a; margin: 0.6in; }}
h1 {{ font-size: 15pt; color: #2c3e50; border-bottom: 2px solid #2c3e50; padding-bottom: 8px; }}
.meta {{ font-size: 9pt; color: #555; margin-bottom: 16px; }}
.footer {{ margin-top: 20px; font-size: 8pt; color: #777; text-align: center; border-top: 1px solid #ccc; padding-top: 8px; }}
</style>
</head>
<body>
<h1>Transcription & Speaker Report</h1>
<div class="meta">
  <strong>Source:</strong> {os.path.basename(source_path)} &nbsp;|&nbsp;
  <strong>Generated:</strong> {os.popen("date -u +%Y-%m-%d\\ %H:%M\\ UTC").read().strip()}
</div>
{lines_html}
<div class="footer">iON Data Security Systems — Hermes Transcription</div>
</body>
</html>"""

    with open(html_path, "w") as f:
        f.write(html)

    subprocess.run([
        "libreoffice", "--headless", "--convert-to", "pdf",
        "--outdir", output_dir, html_path
    ], check=True, capture_output=True)

    return pdf_path


def main():
    if len(sys.argv) < 4:
        print("Usage: hermes_transcribe.py <input_media> <output_dir> <base_name>")
        sys.exit(1)

    input_path = sys.argv[1]
    output_dir = sys.argv[2]
    base_name = sys.argv[3]

    os.makedirs(output_dir, exist_ok=True)

    print(f"[1/4] Extracting audio from {input_path}...")
    wav_path = extract_audio(input_path, output_dir, base_name)

    print(f"[2/4] Transcribing with Whisper small...")
    transcript = transcribe(wav_path, base_name)
    transcript_path = os.path.join(output_dir, f"{base_name}_transcript.json")
    with open(transcript_path, "w") as f:
        json.dump(transcript, f, indent=2)

    print(f"[3/4] Diarizing with pyannote...")
    diarization_output = diarize(wav_path)

    diarization_path = os.path.join(output_dir, f"{base_name}_diarization.txt")
    with open(diarization_path, "w") as f:
        for turn, _, speaker in diarization_output.speaker_diarization.itertracks(yield_label=True):
            f.write(f"{speaker} {turn.start:.3f} {turn.end:.3f}\n")

    print(f"[4/4] Merging and generating PDF...")
    merged = merge(transcript, diarization_output, base_name)
    merged_path = os.path.join(output_dir, f"{base_name}_merged.txt")
    with open(merged_path, "w") as f:
        f.write(merged)

    pdf_path = generate_pdf(merged, output_dir, base_name, input_path)

    print(f"Done. PDF: {pdf_path}")


if __name__ == "__main__":
    main()
