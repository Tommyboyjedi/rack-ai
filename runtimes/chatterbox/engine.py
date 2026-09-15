"""One resident model; ordinary upstream Perth watermarking is mandatory."""
from dataclasses import dataclass
import io
import json
import time
import wave

@dataclass(frozen=True)
class Settings:
    temperature: float = 1.05
    top_p: float = 0.95
    top_k: int = 1000
    repetition_penalty: float = 1.2
    # Pinned NumPy 1.26 retains float32 through upstream loudness normalization.
    norm_loudness: bool = True

class Engine:
    def __init__(self, model_path):
        import numpy as np
        import torch
        import perth
        from chatterbox.tts_turbo import ChatterboxTurboTTS
        if perth.PerthImplicitWatermarker is None:
            raise RuntimeError("perth_watermarker_unavailable")
        self.np, self.torch = np, torch
        self.settings = Settings()
        start = time.monotonic()
        self.model = ChatterboxTurboTTS.from_local(model_path, device="cuda")
        if self.model.sr != 24000:
            raise RuntimeError("unexpected_sample_rate")
        self.voice_digest = None
        self.load_seconds = time.monotonic() - start
        print(json.dumps({"event": "model_loaded", "seconds": self.load_seconds}), flush=True)

    def synthesize(self, request):
        text, voice = request
        start = time.monotonic()
        torch, np = self.torch, self.np
        torch.cuda.reset_peak_memory_stats()
        if voice.digest != self.voice_digest:
            self.model.prepare_conditionals(io.BytesIO(voice.data), norm_loudness=self.settings.norm_loudness)
            self.voice_digest = voice.digest
        with torch.inference_mode():
            audio = self.model.generate(text, temperature=self.settings.temperature,
                top_p=self.settings.top_p, top_k=self.settings.top_k,
                repetition_penalty=self.settings.repetition_penalty)
        samples = audio.detach().cpu().numpy().reshape(-1)
        if not 0 < len(samples) <= 24000 * 60 or not np.isfinite(samples).all():
            raise ValueError("speech_audio_bounds")
        pcm = (np.clip(samples, -1, 1) * 32767).astype("<i2")
        output = io.BytesIO()
        with wave.open(output, "wb") as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(24000)
            wav.writeframes(pcm.tobytes())
        seconds = time.monotonic() - start
        print(json.dumps({"event": "synthesis", "seconds": seconds,
            "audio_seconds": len(samples)/24000, "rtf": seconds/(len(samples)/24000),
            "peak_torch_mib": torch.cuda.max_memory_allocated()/1048576}), flush=True)
        return output.getvalue()
