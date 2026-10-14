"""MkDocs hook: puts the release version (from the Cargo workspace) in the site header and
replaces ``{{ version }}`` in pages, so the docs never state a version by hand."""

import re
from pathlib import Path

_CARGO = Path(__file__).resolve().parent.parent / "Cargo.toml"
VERSION = re.search(r'\[workspace\.package\][^\[]*?version\s*=\s*"([^"]+)"', _CARGO.read_text(), re.S).group(1)


def on_config(config):
    config.site_name = f"{config.site_name} {VERSION}"
    return config


def on_page_markdown(markdown, **kwargs):
    return markdown.replace("{{ version }}", VERSION)
