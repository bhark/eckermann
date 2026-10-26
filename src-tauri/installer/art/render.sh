#!/bin/sh
# renders the installer art from the svg sources and app-icon.png; needs rsvg-convert and imagemagick
set -eu
cd "$(dirname "$0")"

orb=$(mktemp --suffix .png)
trap 'rm -f "$orb"' EXIT
magick ../../../app-icon.png -crop 612x612+205+205 +repage \
  \( -size 612x612 xc:black -fill white -draw "circle 306,306 306,1.5" -blur 0x0.8 \) \
  -alpha off -compose CopyOpacity -composite "$orb"

rsvg-convert dmg.svg -o dmg.png
rsvg-convert -z 2 dmg.svg -o dmg@2x.png

# windows pages at 100 to 200 % scaling; bmp has no alpha, so the orb sits on the page colour
for size in 96 120 144 168 192; do
  magick "$orb" -resize ${size}x$size -background "#1a1a1b" -flatten -type TrueColor BMP3:orb-$size.bmp
done
