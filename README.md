# Earmark

An EQ earmarked for your ears.

A Windows desktop app (Rust + egui) that builds a personal headphone EQ from a short hearing test,
then lets you hear it on your own music and copy it into the EQ you already use.

1. **Prepare**: set a comfortable volume with a 1 kHz tone, check left and right, and choose the
   test: one ear at a time (a separate EQ for each ear) or both ears together (half the time).
2. **Test**: short beeps at different pitches. Answer *heard* / *not heard*, or replay a tone (R).
   Each pitch and ear runs its own Hughson-Westlake-style staircase (down 10 dB when heard, up 5 dB
   when missed; the threshold is two hits on the way up within 5 dB of each other). Three pitches
   run at a time in random order with random gaps, so they finish steadily; a progress bar and a
   time-left estimate based on your own pace show how far through you are. Silent checks flag
   guessing. Once some pitches are measured, each new one starts 5 dB above the level predicted
   from those already done.
3. **Tune**: thresholds minus the ISO 226 normal-hearing curve give what's specific to you and
   your headphones. The median is removed, the result is scaled by *strength* and limited by max
   boost and cut, then fitted with low-shelf, peak and high-shelf filters, with a preamp so nothing
   clips.
4. **Use**: A/B the EQ at matched levels on pink noise or your own music (MP3, FLAC, M4A/AAC, OGG
   or WAV; add files or drag them onto the window). Then copy it into the EQ you already use:
   slider values for Spotify, Sony Headphones Connect, a 10-band graphic EQ or your own band
   frequencies, a filter table for parametric EQ apps, or export files (a per-ear config, and
   AutoEQ-style `ParametricEQ.txt` / `GraphicEQ.txt`).

## Not a medical test

Earmark tunes how your headphones sound to you. It is not a medical device or a clinical hearing
test, and it can't diagnose hearing loss. Levels aren't calibrated in dB HL, and results depend on
your headphones, their fit and the room. If you've noticed a change in your hearing, or one ear is
much weaker than the other, see a doctor or audiologist.

## How the EQ is made

The test measures your ears *and* your headphones together, at the quietest audible level. That's
why the EQ is applied at partial strength: differences shrink at normal listening volume.

## Files

- Settings and results: `%APPDATA%\Earmark\data\app.ron`.
- Crash reports: `%APPDATA%\Earmark\data\crash.log`. If Earmark crashes it says where the report
  is; please include it with any bug report.

## Building

Needs Rust and the MSVC build tools (the Windows SDK's `rc.exe` embeds the icon and version info).

```sh
cargo run --release
cargo test
cargo test screenshots -- --ignored   # renders every screen, light and dark, to %TEMP%\earmark-*.png
cargo test write_ico -- --ignored     # regenerates assets/earmark.ico after changing the icon
cargo about generate about.hbs -o THIRD_PARTY_LICENSES.html   # after changing dependencies
```

## Licence

Earmark is licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at
your option.

It's built on open-source crates under their own licences, listed with their full texts in
[THIRD_PARTY_LICENSES.html](THIRD_PARTY_LICENSES.html); include that file when distributing
`earmark.exe`. Audio decoding uses [Symphonia](https://github.com/pdeljanov/Symphonia), unmodified,
under the Mozilla Public License 2.0; its source is available from that repository and crates.io.
Earmark uses the Segoe UI fonts and Segoe Fluent Icons installed with Windows at runtime and doesn't
include them.
