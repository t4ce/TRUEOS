#!/usr/bin/env python3
"""Export Old-Games.com's public game-page URLs from its official sitemap.

This script exports catalog pages such as:
    https://www.old-games.com/download/2618/pac-girl

It deliberately does not discover /getfile/ URLs, submit access codes, or
download game files.
"""

from __future__ import annotations

import argparse
import re
import sys
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path
from urllib.parse import urlsplit


DEFAULT_SITEMAP = "https://www.old-games.com/sitemap.xml"
DEFAULT_OUTPUT = Path("old_games_catalog_links.txt")
USER_AGENT = "OldGamesPublicCatalogExporter/1.0"
GAME_PAGE_PATH = re.compile(r"^/download/[0-9]+/[^/]+/?$")


def fetch_sitemap(url: str, timeout: float) -> bytes:
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/xml,text/xml;q=0.9,*/*;q=0.1",
            "User-Agent": USER_AGENT,
        },
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return response.read()


def extract_game_pages(xml_data: bytes) -> list[str]:
    root = ET.fromstring(xml_data)
    links: list[str] = []
    seen: set[str] = set()

    # The namespace can change without affecting extraction because ElementTree's
    # wildcard matches every namespace used for the standard sitemap `loc` tag.
    for element in root.findall(".//{*}loc"):
        if not element.text:
            continue

        url = element.text.strip()
        parsed = urlsplit(url)
        if (
            parsed.scheme == "https"
            and parsed.hostname == "www.old-games.com"
            and not parsed.query
            and not parsed.fragment
            and GAME_PAGE_PATH.fullmatch(parsed.path)
            and url not in seen
        ):
            seen.add(url)
            links.append(url)

    return links


def write_links(path: Path, links: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(f"{link}\n" for link in links), encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Export public Old-Games.com game-page links from the site's "
            "official sitemap. No game files are downloaded."
        )
    )
    parser.add_argument(
        "-o",
        "--output",
        type=Path,
        default=DEFAULT_OUTPUT,
        help=f"output text file (default: {DEFAULT_OUTPUT})",
    )
    parser.add_argument(
        "--sitemap",
        default=DEFAULT_SITEMAP,
        help=f"sitemap URL (default: {DEFAULT_SITEMAP})",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=30.0,
        help="network timeout in seconds (default: 30)",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.timeout <= 0:
        print("error: --timeout must be greater than zero", file=sys.stderr)
        return 2

    try:
        xml_data = fetch_sitemap(args.sitemap, args.timeout)
        links = extract_game_pages(xml_data)
    except (urllib.error.URLError, TimeoutError) as error:
        print(f"error: could not fetch sitemap: {error}", file=sys.stderr)
        return 1
    except ET.ParseError as error:
        print(f"error: invalid sitemap XML: {error}", file=sys.stderr)
        return 1

    if not links:
        print("error: sitemap contained no matching game-page URLs", file=sys.stderr)
        return 1

    write_links(args.output, links)
    print(f"Wrote {len(links):,} public game-page URLs to {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
