# mr

A live-coding CLI for Ableton Live. Pipe-based, composable, fast.

`mr` turns your terminal into a musical instrument. Generators produce patterns, transforms reshape them, and sinks write them into a running Ableton session — all connected with Unix pipes.

```bash
mr scale C4 dorian --octaves 2 | mr degrade 0.15 | mr overlap 0.3 | mr write pad:0 --length 17
```

## How it works

Four JSON data types flow through pipes:

- **Pattern** — notes with pitch, time, duration, velocity
- **Progression** — sequences of chords
- **Curve** — automation envelopes (arrays of floats)
- **Pitches** — raw pitch lists

Generators emit data to stdout. Transforms read stdin and write stdout. Sinks consume stdin and send OSC to Ableton. Chain them freely.

## Commands

### Session control
```bash
mr tempo 128            # set tempo
mr play                 # start transport
mr stop                 # stop transport
mr status               # tempo, track count, playing state
mr undo / mr redo
```

### Tracks & mixing
```bash
mr track pad --simpler ~/samples/glass.wav    # create track with sample
mr track bass --instrument Analog             # create track with synth
mr effect pad "Auto Filter"                   # add audio effect
mr mix pad --vol 0.7 --pan=-0.3               # set levels
mr send pad reverb 0.4                        # send to return track
mr device pad "Filter Freq":0.6 --device 1    # set device parameter
```

### Inspection
```bash
mr tracks               # list all tracks with levels
mr inspect pad          # devices, clips, sends
mr params pad           # all device parameter names and values
mr read pad:0           # clip notes as JSON (round-trip ready)
mr meters               # real-time output levels
mr returns              # return tracks and devices
```

### Pattern generators
```bash
mr scale C4 dorian --octaves 2 --duration 0.25
mr euclidean 7 16 --pitch 36 --duration 0.25
mr chord Cmaj7 --duration 4
mr notes "C4:2 Eb4:1 G4:0.5"
mr markov C4 dorian --steps 32
mr accumulate --seed '...' --transforms "rotate:3,transpose:5" --iterations 8
```

### Pattern transforms
```bash
mr transpose 5          # shift pitch
mr rotate 3             # rotate events
mr retrograde           # reverse time
mr invert               # mirror pitch around axis
mr stretch 0.5          # compress/expand time
mr degrade 0.2          # randomly remove notes
mr thin 1.0             # remove by duration threshold
mr humanize             # add timing/velocity variation
mr swing 0.33           # triplet swing
mr overlap 0.3          # extend note durations
mr crescendo 40 120     # velocity ramp
mr quantize 0.25        # snap to grid
mr repeat 4             # repeat pattern
mr velocities "100 60 80 60"  # apply velocity pattern
mr morph second.json --amount 0.6   # interpolate between two patterns
mr weave --voice a --balance 0.5    # split into complementary voices
```

### Curves & automation
```bash
# Generate
mr curve sine --range 0.2 0.8 --bars 8 --cycle 2
mr curve drunk --range 0.3 0.7 --bars 16 --step 0.04
mr curve envelope --attack 0.1 --sustain 0.6 --release 0.3 --bars 8
mr curve ramp --from 0.0 --to 1.0 --bars 4
mr curve stochastic --range 0.1 0.9 --density 0.3 --bars 16

# Transform
mr curve-invert           # flip vertically
mr curve-reverse          # flip horizontally
mr curve-scale 0.3 0.7    # remap range
mr curve-offset 0.1       # shift values
mr curve-blend other.json --amount 0.5

# Apply to Ableton
mr automate pad:0 "Filter Freq"
mr automate pad:0 "Filter Freq" --device 1   # effect device
```

### Harmony
```bash
mr harmony walk Cmaj7 --steps 8 --dist 1    # random walk through chord space
mr harmony path Cmaj7 F#minmaj7 --smoothest  # find voice-leading path
mr harmony orbit Cmajor P,L,R               # PLR cycle (triadic)
mr voice bloom --range 48 72                 # voice a progression
mr voice open / wide / led                   # voicing strategies
```

### Real-time parameter control
```bash
mr walk pad "Filter Freq" --from 0.2 --to 0.8 --seconds 8           # linear ramp
mr walk pad "Filter Freq" --drunk --range 0.3 0.7 --step 0.05 --seconds 20  # brownian
mr walk pad "Filter Freq" --sine --range 0.2 0.6 --cycle 4 --seconds 16     # oscillate
```

### Genre & rhythm
```bash
mr genre list                                 # 18 genres with BPM ranges
mr genre drums house --tempo 124              # canonical drum pattern
mr rhythm four-on-the-floor --pitch 36        # rhythmic figure
mr rhythm dembow / 2step / breakbeat / clave-32
```

### Cross-domain bridges
```bash
# Extract features from a pattern as a curve
mr read pad:0 | mr to-curve pitch --bars 16

# Convert curve values to pitches
mr curve drunk --range 48 72 --bars 8 | mr to-pattern
```

### Persistence
```bash
mr save my_groove              # save to ~/.mr/stash/
mr load my_groove              # recall
mr snapshot save mix_v1        # save full session mix state
mr snapshot restore mix_v1     # recall mix state
```

## Architecture

```
mr (Rust CLI)
  ├── metaritual (pattern algebra, harmony, transforms)
  ├── ableton-rs (OSC transport)
  └── AbletonOSC (Ableton Remote Script)
        └── Ableton Live
```

`mr` depends on [metaritual](https://github.com/hhhults/metaritual) for music theory and pattern operations, and [ableton-rs](https://github.com/hhhults/ableton-rs) for OSC communication with Ableton Live via the [AbletonOSC](https://github.com/ideoforms/AbletonOSC) remote script.

## Building

```bash
cargo build --release
cargo install --path .
```

Requires a running Ableton Live instance with AbletonOSC installed.

## License

MIT
