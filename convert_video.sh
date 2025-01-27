#!/usr/bin/env bash

set -eu
set -o pipefail

mkdir -p /tmp/pxlsvideoconv_original/
mkdir -p /tmp/pxlsvideoconv_conv/

ffmpeg -i "$1" /tmp/pxlsvideoconv_original/image-%5d.jpeg

declare -i i=0

for image in /tmp/pxlsvideoconv_original/*.jpeg; do
  printf -v filename "/tmp/pxlsvideoconv_conv/%05d.jpeg" $i
  pxls "$image" 100 10 euclidean "$filename" "$2" 4 2

  i+=1
done

rm -r /tmp/pxlsvideoconv_original
ffmpeg -framerate 30 -pattern_type glob -i "/tmp/pxlsvideoconv_conv/*.jpeg" "$3"
rm -r /tmp/pxlsvideoconv_conv/