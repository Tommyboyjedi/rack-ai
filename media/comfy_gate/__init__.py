"""Pinned ComfyUI custom-node entrypoint. No nodes, frontend modifications or models."""
from .gate import install
from server import PromptServer

install(PromptServer.instance.app)
NODE_CLASS_MAPPINGS = {}
