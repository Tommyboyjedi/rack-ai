# Dual-service image support

The `dual-brain` and `dual-worker` profiles add visual interpretation to their
existing managed Chat Completions interface. Ordinary clients reserve either
logical service, hold its returned scoped gateway, send text/image content parts,
and release through the standard runtime contract. No consumer-specific API or
extra GPU ownership route is introduced.

Both preserve context/input/output limits of 131072/65536/65536 tokens, one
parallel slot and the original text model weights. Image preprocessing is limited
to 1024 encoder tokens per image. Request limits are two images, 256 KiB decoded
bytes per image, 4 Mi pixels per image and 1 MiB for the complete HTTP body.
`dual-brain` accepts PNG, JPEG, WebP, GIF and BMP; `dual-worker` accepts PNG,
JPEG, GIF and BMP through its explicit typed `image_mime_types` subset.
Only inline base64 data URLs are accepted; image detail must be absent or `auto`.
Images belong in user messages. See discovery for the accepted MIME types and
[dual-services.md](dual-services.md) for thinking-off and structured-output controls.

## Native counting and ownership

The optional typed `input_token_counter: "native_multimodal"` selector enables
image-aware preflight and mandatory admission. Other profiles retain their
existing counter, serialized identities and behavior. A counter error fails the
call before generation. Images consume the same frozen input/context budget as
text, schemas, tools and conversation history.

Nex uses the installed llama.cpp `/v1/chat/completions/input_tokens` endpoint.
It applies the actual Chat Completions parser and native multimodal preprocessing
without neural encoding or generation. Readiness additionally requires a loaded
vision modality, the configured context and one slot. The separate F16 projector
runs on the CPU within the owned service cgroup. Native codec lookup is bound
to `/dev/null` with `--video-ffmpeg-dir /dev/null`, so unsupported or malformed
images cannot launch FFmpeg/FFprobe through an unqualified fallback.

Strata uses the prepared CPU vision helper and a small reviewable COUNT protocol
patch. COUNT and image encoding share native mtmd tokenization; the counter expands
the actual template's image markers and includes any forced-tool prefix. The CPU
helper is pinned and checked as an owned child, and cannot restart inside an
activation. Failed or ambiguous helper I/O invalidates the helper instead of
reusing a potentially stale protocol response. Normal service release stops the
complete cgroup. The existing image cache is an LRU of 64 entries (640 MiB of retained
encoded files at this profile, plus bounded files used during an encode), within
a private generation temporary directory. Normal shutdown removes it. An abrupt SIGKILL can leave that
bounded directory for operator cleanup; its exact path is retained in status.
Host and GPU admission allowances are unchanged.

## Projector provenance

- Nex: [publisher and conversion provenance](https://huggingface.co/ngquocvinh/Nex-N2.5-mini-GGUF),
  revision `fe3093c81ec10e0388c5375468d2b6a86e0a6026`,
  `mmproj-Nex-N2.5-mini-F16.gguf`, 899283040 bytes,
  SHA256 `c128ba00f092ecc75a15fbc31174588faac26cbfbc3d0bff4b828f73f69604e4`.
- Strata: [original BF16 projector](https://huggingface.co/ISTA-DASLab/Qwen3.8-Flash-Next-GSQ-RCO-GGUF),
  `mmproj-Qwen3.8-Flash-Next-BF16.gguf`, 907543008 bytes,
  SHA256 `b1a82259702816a5330d7bd7607cd9676b11780e79ff7348c21103ff3ce49bd0`.

Projectors are prepared and hashed before activation; activation performs no
network downloads or compilation. The original models and serving installations
are retained. Installation profiles remain unqualified until managed functional
validation succeeds.

## Validation

The bounded live proof uses synthetic images with different shapes/colors, plus
a two-image request. Expected answers are derived from fixture construction and
are absent from the prompts. It checks exact preflight versus actual prompt
usage, schema output, thinking-off operation, reservation holding and full
owned-process release. Text/reasoning/schema/tool regressions run through the same
managed gateway. This is functional qualification, not a general visual-aptitude
or maximum-window performance evaluation.
