# Fichiers de test

Sinus de 1 kHz, 1 s, mono, 44,1 kHz, générés localement le 2026-11-02 par FFmpeg 9.0.2 (rien de téléchargé). Signal synthétique sans ayant droit : CC0-1.0.

| Fichier | Commande |
|---|---|
| `tone.flac` | `ffmpeg -f lavfi -i "sine=frequency=1000:sample_rate=44100:duration=1" -c:a flac tone.flac` |
| `tone.mp3` | `ffmpeg -f lavfi -i "sine=frequency=1000:sample_rate=44100:duration=1" -c:a libmp3lame -b:a 64k tone.mp3` |
| `tone.ogg` | `ffmpeg -f lavfi -i "sine=frequency=1000:sample_rate=44100:duration=1" -c:a libvorbis -q:a 1 tone.ogg` |
| `tone.m4a` | `ffmpeg -f lavfi -i "sine=frequency=1000:sample_rate=44100:duration=1" -c:a aac -b:a 64k tone.m4a` |

`tone_video.mp4` : mp4 non fragmenté à deux pistes (vidéo H.264 64x64 grise fixe et audio AAC stéréo 1 kHz), 6 s, 41 640 octets, moov en tête, pistes entrelacées. Même origine et même licence que ci-dessus.

`ffmpeg -f lavfi -i "color=c=gray:s=64x64:r=10" -f lavfi -i "sine=frequency=1000:sample_rate=44100" -t 6 -c:v libx264 -tune stillimage -preset veryslow -crf 40 -pix_fmt yuv420p -c:a aac -b:a 48k -ac 2 -movflags +faststart tone_video.mp4`

`hls/` : sinus 1 kHz stéréo de 3 s en segments fMP4 AAC 32 kbit/s (`init.mp4`, `seg0..3.m4s`) et sa playlist `tone.m3u8`, même structure que les flux HLS de SoundCloud. Même origine et même licence que ci-dessus. Durées EXTINF : 1,021678 / 0,998458 / 0,998458 / 0,023220 s.

`ffmpeg -f lavfi -i "sine=frequency=1000:sample_rate=44100:duration=3" -ac 2 -c:a aac -b:a 32k -f hls -hls_time 1 -hls_segment_type fmp4 -hls_fmp4_init_filename init.mp4 -hls_segment_filename "seg%d.m4s" -hls_playlist_type vod tone.m3u8`
