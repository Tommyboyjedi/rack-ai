"""CPU-only checks for bounded image parsing, exact expansion and helper fencing."""
import base64
import io
from pathlib import Path
import sys
import threading
from types import SimpleNamespace

import pytest
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "runtime_adapters"))
from strata_images import InlineImage, InputMessages
from strata_vision_count import ImageTokenCounter
from strata_guard import ActivationGuard
from strata_vision import VisionConfiguration


def inline(format="PNG", size=(24, 16)):
    output = io.BytesIO()
    Image.new("RGB", size, "red").save(output, format=format)
    return f"data:image/{format.lower()};base64," + base64.b64encode(output.getvalue()).decode()


def request(*sources, role="user", detail="auto"):
    return {"messages": [{"role": role, "content": [
        {"type": "text", "text": "Describe the pixels."},
        *[{"type": "image_url", "image_url": {"url": source, "detail": detail}} for source in sources],
    ]}]}


@pytest.mark.parametrize("format", ["PNG", "JPEG", "WEBP", "GIF", "BMP"])
def test_all_advertised_formats_decode_and_support_string_or_object_image_url(format):
    source = inline(format)
    assert InlineImage.decode(source)
    InputMessages.validate(request(source))
    InputMessages.validate({"messages": [{"role": "user", "content": [{"type": "image_url", "image_url": source}]}]})


@pytest.mark.parametrize("source", ["https://example.invalid/x.png", "file:///etc/passwd", "/etc/passwd",
                                      "data:image/png;base64,??", "data:image/png;base64,AAAA"])
def test_external_local_malformed_and_unreadable_images_fail_closed(source):
    with pytest.raises(ValueError):
        InputMessages.validate(request(source))


def test_image_mime_pixel_byte_count_role_and_detail_limits():
    source = inline()
    cases = [request(source.replace("image/png", "image/jpeg")), request(inline(size=(3000, 1500))),
             request(source, source, source), request(source, role="assistant"), request(source, detail="high"),
             request("data:image/png;base64," + "A" * 350000)]
    for body in cases:
        with pytest.raises(ValueError):
            InputMessages.validate(body)


def vision_fixture(tmp_path, response="COUNT 6 3 2"):
    events = []
    process = SimpleNamespace(stdin=io.StringIO(), kill=lambda: events.append("kill"),
                              wait=lambda timeout: events.append("wait"))
    vision = SimpleNamespace(proc=process, dir=tmp_path, lock=threading.Lock(), stopped=False,
                             rackai_count_protocol=True, cache={}, normalize=lambda value: value)
    vision.alive = lambda: not vision.stopped
    vision._readline = lambda timeout, what: response
    runtime = SimpleNamespace(images_of=lambda messages: messages, IMAGE_PAD="pad", VISION_START="start")
    tokenizer = SimpleNamespace(encode=lambda text, parse_special: [90 if text == "pad" else 80]
                                if parse_special else [7, 8, 9])
    return ImageTokenCounter(SimpleNamespace(vision=vision, tok=tokenizer), runtime), events


def test_native_counts_expand_each_marker_preserve_literals_and_leave_no_embedding_cache(tmp_path):
    counter, events = vision_fixture(tmp_path)
    # Text + template image pair + literal pad + second template image pair.
    ids = [1, 80, 90, 2, 90, 3, 80, 90, 4]
    assert counter.expanded_count(ids, [inline(), inline()]) == 21
    assert counter.service.vision.proc.stdin.getvalue().count("COUNT ") == 2
    assert not counter.service.vision.cache and not list(tmp_path.iterdir()) and not events


@pytest.mark.parametrize("response", ["COUNT 6 2 2", "OK 6 3 2", "COUNT nonsense 3 2", ""])
def test_invalid_native_reply_permanently_fences_helper(tmp_path, response):
    counter, events = vision_fixture(tmp_path, response)
    with pytest.raises(ValueError, match="new activation"):
        counter.image_tokens(inline())
    before = counter.service.vision.proc.stdin.getvalue()
    with pytest.raises(ValueError):
        counter.expanded_count([80, 90], [inline()])
    assert counter.service.vision.proc.stdin.getvalue() == before
    assert events == ["kill", "wait"] and not list(tmp_path.iterdir())


def test_timeout_cannot_poison_a_later_count_with_a_delayed_reply(tmp_path):
    counter, events = vision_fixture(tmp_path)
    def timeout(*args):
        raise RuntimeError("timed out")
    counter.service.vision._readline = timeout
    with pytest.raises(ValueError, match="new activation"):
        counter.image_tokens(inline())
    counter.service.vision._readline = lambda *args: "COUNT 1 1 1"
    with pytest.raises(ValueError, match="unavailable"):
        counter.expanded_count([80, 90], [inline()])
    assert counter.service.vision.proc.stdin.getvalue().count("COUNT ") == 1
    assert events == ["kill", "wait"]


def test_template_image_mismatch_is_rejected(tmp_path):
    counter, _ = vision_fixture(tmp_path)
    with pytest.raises(ValueError, match="do not match"):
        counter.expanded_count([80, 90, 80, 90], [inline()])
    with pytest.raises(ValueError, match="do not match"):
        counter.expanded_count([1, 2], [inline()])


def test_activation_guards_the_encoder_and_rejects_replacement_or_native_restart():
    import os
    def child():
        return SimpleNamespace(proc=SimpleNamespace(pid=os.getpid(), poll=lambda: None), alive=lambda: True,
                               rackai_count_protocol=True)
    service = SimpleNamespace(engine=child(), vision=child())
    guard = ActivationGuard(service, SimpleNamespace(EngineDied=RuntimeError))
    guard.check()
    for action in (service.vision.restart, service.vision.unload):
        with pytest.raises(RuntimeError, match="new RackAI activation"):
            action()
    service.vision = child()
    with pytest.raises(RuntimeError, match="generation changed"):
        guard.check()


def test_vision_requires_qualified_initial_handshake_and_explicit_cpu_environment():
    class Vision:
        _readline = lambda *args: "READY 2560 RACKAI_COUNT_V1\n"
    runtime = SimpleNamespace(Vision=Vision, vision_env=lambda config, env: dict(env))
    VisionConfiguration.install(runtime)
    vision = Vision()
    assert vision._readline(1, "ready").startswith("READY") and vision.rackai_count_protocol
    assert runtime.vision_env({"vision": {}}, {"CUDA_VISIBLE_DEVICES": "GPU-a,GPU-b"})["CUDA_VISIBLE_DEVICES"] == "GPU-a,GPU-b"
    assert runtime.vision_env({"vision": {"gpu": False}}, {"CUDA_VISIBLE_DEVICES": "GPU-a,GPU-b"})["CUDA_VISIBLE_DEVICES"] == "-1"
    class WrongVision:
        _readline = lambda *args: "READY 2560\n"
    runtime.Vision = WrongVision
    VisionConfiguration.install(runtime)
    with pytest.raises(RuntimeError, match="lacks exact"):
        WrongVision()._readline(1, "ready")


def test_configuration_rejects_elastic_gpu_vision_and_unprepared_artifacts(tmp_path):
    artifact = tmp_path / "file"
    artifact.write_bytes(b"prepared")
    config = {"args": ["--vision"], "vision": {"exe": str(artifact), "mmproj": str(artifact),
              "model": str(artifact), "gpu": False, "threads": 16, "max_tokens": 1024}}
    VisionConfiguration.validate(config)
    for replacement in ({"gpu": True}, {"elastic": True}, {"threads": 32}, {"max_tokens": 300},
                        {"exe": "/nonexistent/rackai-vision"}):
        with pytest.raises(ValueError):
            VisionConfiguration.validate({**config, "vision": {**config["vision"], **replacement}})
    with pytest.raises(ValueError, match="enable vision"):
        VisionConfiguration.validate({**config, "args": []})
