"""Bound inline image decoding before any native media processing."""
import base64
import binascii
import io
import re
import warnings

MAX_IMAGES = 2
MAX_IMAGE_BYTES = 262144
MAX_IMAGE_PIXELS = 4194304
MIME_FORMATS = {"png": "PNG", "jpeg": "JPEG", "webp": "WEBP", "gif": "GIF", "bmp": "BMP"}


class InlineImage:
    @staticmethod
    def decode(source):
        if not isinstance(source, str):
            raise ValueError("image_url.url must be an inline image data URL")
        match = re.fullmatch(r"data:image/(png|jpeg|webp|gif|bmp);base64,([A-Za-z0-9+/=]+)", source)
        if match is None or len(match[2]) > ((MAX_IMAGE_BYTES + 2) // 3) * 4:
            raise ValueError("send a bounded inline PNG, JPEG, WebP, GIF or BMP image")
        try:
            data = base64.b64decode(match[2], validate=True)
        except (ValueError, binascii.Error):
            raise ValueError("image base64 is invalid") from None
        if not data or len(data) > MAX_IMAGE_BYTES:
            raise ValueError("decoded image exceeds the byte limit")
        from PIL import Image
        try:
            with warnings.catch_warnings():
                warnings.simplefilter("error", Image.DecompressionBombWarning)
                with Image.open(io.BytesIO(data), formats=[MIME_FORMATS[match[1]]]) as image:
                    if image.format != MIME_FORMATS[match[1]]:
                        raise ValueError("image MIME type does not match its bytes")
                    if image.width <= 0 or image.height <= 0 or image.width * image.height > MAX_IMAGE_PIXELS:
                        raise ValueError("image dimensions exceed the pixel limit")
                    image.load()
        except (OSError, ValueError, Image.DecompressionBombWarning, Image.DecompressionBombError):
            raise ValueError("image is invalid or exceeds the pixel limit") from None
        return data


class InputMessages:
    @staticmethod
    def validate(request):
        if not isinstance(request, dict):
            raise ValueError("send a JSON object")
        if request.get("strata_mcp") or request.get("strata_prefix") is not None:
            raise ValueError("only stateless Chat Completions are supported")
        messages = request.get("messages")
        if not isinstance(messages, list) or not messages:
            raise ValueError("messages must be a nonempty array")
        count = 0
        for message in messages:
            if not isinstance(message, dict):
                raise ValueError("messages must contain objects")
            content = message.get("content")
            if content is None or isinstance(content, str):
                continue
            if not isinstance(content, list) or not content:
                raise ValueError("content must be a string or nonempty parts array")
            for part in content:
                if not isinstance(part, dict):
                    raise ValueError("content parts must be objects")
                if part.get("type") == "text" and isinstance(part.get("text"), str):
                    continue
                image = part.get("image_url")
                if isinstance(image, str):
                    image = {"url": image}
                if part.get("type") != "image_url" or message.get("role") != "user" or not isinstance(image, dict):
                    raise ValueError("only user inline image parts and text are supported")
                if image.get("detail", "auto") != "auto":
                    raise ValueError("only automatic image detail is supported")
                count += 1
                if count > MAX_IMAGES:
                    raise ValueError("request exceeds the image count limit")
                InlineImage.decode(image.get("url"))
