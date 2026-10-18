"""Qualified CPU encoder configuration and ready protocol fencing."""
from pathlib import Path
import importlib


class VisionConfiguration:
    @staticmethod
    def validate(config):
        vision = config.get("vision")
        if vision is None:
            return
        fields = {"exe", "mmproj", "model", "gpu", "threads", "max_tokens"}
        if not isinstance(vision, dict) or set(vision) != fields:
            raise ValueError("only the qualified resident CPU vision configuration is supported")
        if vision["gpu"] is not False or vision["threads"] != 16 or vision["max_tokens"] != 1024:
            raise ValueError("vision requires the qualified CPU placement and image budget")
        if "--vision" not in config.get("args", []):
            raise ValueError("the native engine must enable vision explicitly")
        for key in ("exe", "mmproj", "model"):
            path = Path(vision[key])
            if not path.is_absolute() or not path.is_file():
                raise ValueError("vision executable and artifacts must be prepared absolute paths")
        # Register only the five supported decoders and their normalization dependencies.
        for module in ("BmpImagePlugin", "GifImagePlugin", "JpegImagePlugin", "PngImagePlugin",
                       "WebPImagePlugin", "ImageOps", "TiffImagePlugin"):
            importlib.import_module(f"PIL.{module}")
        from PIL import Image
        if not hasattr(Image, "open"):
            raise ValueError("vision image decoding is unavailable")

    @staticmethod
    def install(runtime):
        original = runtime.Vision._readline
        original_env = runtime.vision_env

        def read(vision, timeout, what):
            line = original(vision, timeout, what)
            if line.startswith("READY"):
                fields = line.strip().split()
                vision.rackai_count_protocol = len(fields) == 3 and fields[2] == "RACKAI_COUNT_V1"
                if not vision.rackai_count_protocol:
                    raise RuntimeError("the image encoder lacks exact managed counting")
            return line

        def environment(config, values):
            result = original_env(config, values)
            if config.get("vision"):
                result = {**result, "CUDA_VISIBLE_DEVICES": "-1"}
            return result

        runtime.Vision._readline = read
        runtime.vision_env = environment
