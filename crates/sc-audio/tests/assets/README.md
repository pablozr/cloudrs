# Test assets

Short sine tones generated with ffmpeg, used by `tests/decode.rs`:

```sh
# fMP4 AAC HLS (the shape of SoundCloud's current streams), 3 s, 44.1 kHz stereo
ffmpeg -f lavfi -i "sine=frequency=440:duration=3:sample_rate=44100" -ac 2 -c:a aac -b:a 64k \
  -f hls -hls_time 1 -hls_segment_type fmp4 -hls_fmp4_init_filename init.mp4 \
  -hls_segment_filename "seg%d.m4s" -hls_playlist_type vod fmp4.m3u8
# ADTS AAC HLS, 2 s
ffmpeg -f lavfi -i "sine=frequency=330:duration=2:sample_rate=44100" -ac 2 -c:a aac -b:a 64k \
  -f segment -segment_format adts -segment_time 1 -segment_list adts.m3u8 -segment_list_type m3u8 "adts%d.aac"
# Progressive MP3, 1 s, 48 kHz mono
ffmpeg -f lavfi -i "sine=frequency=550:duration=1:sample_rate=48000" -ac 1 -c:a libmp3lame -b:a 64k tone.mp3
```
