#!/usr/bin/env python3
"""Generate native app icons from one square PNG. Requires Pillow only when regenerating."""
import argparse
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, default=ROOT / 'assets/icons/opterm.png')
    parser.add_argument('--create-default', action='store_true', help='Create the initial Opterm artwork')
    args = parser.parse_args()
    output = ROOT / 'assets/icons'
    output.mkdir(parents=True, exist_ok=True)
    if args.create_default:
        if args.source.exists():
            parser.error('The source already exists; preserve or remove it explicitly first')
        image = Image.new('RGBA', (1024, 1024))
        draw = ImageDraw.Draw(image)
        draw.rounded_rectangle((40, 40, 984, 984), radius=220, fill='#171d2b')
        draw.rounded_rectangle((106, 164, 918, 858), radius=110, fill='#24324b')
        draw.line([(262, 354), (428, 512), (262, 670)], fill='#70b6ff', width=76, joint='curve')
        draw.rounded_rectangle((502, 640, 768, 708), radius=24, fill='#f0f5ff')
        image.save(args.source)
    with Image.open(args.source) as source:
        if source.width != source.height or source.width < 1024:
            parser.error('Use a square PNG at least 1024 × 1024 pixels')
        source = source.convert('RGBA')
        source.resize((256, 256), Image.Resampling.LANCZOS).save(output / 'window-icon.png')
        source.save(output / 'opterm.ico', sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)])
        source.save(output / 'opterm.icns', sizes=[(n, n) for n in (16, 32, 64, 128, 256, 512, 1024)])
    print('Updated window-icon.png, opterm.ico and opterm.icns in', output)


if __name__ == '__main__':
    main()
