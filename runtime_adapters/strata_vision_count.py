"""Exact native image preprocessing counts, without encoder or model inference."""
from dataclasses import dataclass
import tempfile
import subprocess
from pathlib import Path

from strata_images import InlineImage

COUNT_TIMEOUT_SECONDS = 5


@dataclass(frozen=True)
class ImageTokenCounter:
    service: object
    runtime: object

    def expanded_count(self, ids, messages):
        sources = self.runtime.images_of(messages)
        if not sources:
            return len(ids)
        vision = self.service.vision
        if vision is None or not getattr(vision, "rackai_count_protocol", False) or not vision.alive() or getattr(vision, "rackai_failed", False):
            raise ValueError("managed vision counting is unavailable")
        counts = [self.image_tokens(source) for source in sources]
        pad = self.service.tok.encode(self.runtime.IMAGE_PAD, parse_special=True)[0]
        start = self.service.tok.encode(self.runtime.VISION_START, parse_special=True)[0]
        literal = self.service.tok.encode(self.runtime.IMAGE_PAD, parse_special=False)
        total, used = 0, 0
        for index, token in enumerate(ids):
            if token == pad and index > 0 and ids[index - 1] == start:
                if used >= len(counts):
                    raise ValueError("the prompt and its images do not match")
                total += counts[used]
                used += 1
            else:
                total += len(literal) if token == pad else 1
        if used != len(counts):
            raise ValueError("the prompt and its images do not match")
        return total

    def image_tokens(self, source):
        vision = self.service.vision
        data = vision.normalize(InlineImage.decode(source))
        if not vision.lock.acquire(timeout=COUNT_TIMEOUT_SECONDS):
            raise ValueError("managed vision counter is busy")
        path = None
        try:
            if not vision.alive() or getattr(vision, "rackai_failed", False):
                raise ValueError("managed vision counter stopped")
            with tempfile.NamedTemporaryFile(dir=vision.dir, prefix="count-", suffix=".img", delete=False) as image:
                path = Path(image.name)
                image.write(data)
            vision.proc.stdin.write(f"COUNT {path.name}\n")
            vision.proc.stdin.flush()
            line = vision._readline(COUNT_TIMEOUT_SECONDS, "image count timed out").strip().split()
            if len(line) != 4 or line[0] != "COUNT":
                raise ValueError("native image counting failed")
            count, width, height = (int(value) for value in line[1:])
            if min(count, width, height) <= 0 or width * height != count:
                raise ValueError("native image grid is invalid")
            return count
        except (OSError, RuntimeError, ValueError) as error:
            # A late reply must never be consumed by the next count or encode.
            vision.rackai_failed, vision.stopped = True, True
            if vision.proc is not None:
                try:
                    vision.proc.kill()
                    vision.proc.wait(timeout=1)
                except (OSError, subprocess.TimeoutExpired):
                    pass
            raise ValueError("native image counting is unavailable; a new activation is required") from error
        finally:
            if path is not None:
                path.unlink(missing_ok=True)
            vision.lock.release()
