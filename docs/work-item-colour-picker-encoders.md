# Work item: drive the colour picker's encoders from the surfaces

Raised by the KB-24 hardware pass on 2026-10-11 ([M-Touch record](probes/kb-24-hardware-mtouch-macos.md), finding 1).
Not started. This is a new feature, not a defect of the shipped behaviour.

## What the operator expects

With onPC's Color Picker open, the console's encoder bar shows the picker's encoders (x, y, brightness, colour
space), and they are editable on the console. The operator expects the NX-K rotaries and the M-Touch parameter strips
to drive them too: "there is no reason to not be able to change them".

## What happens today

- The feedback module reads the encoder bar's attribute slots from its `PresetBar`. With the picker open the bar holds
  a `ColorEditBar` instead, so the slots are unavailable. Since feedback 0.7.1 the reason says so: "display 1 shows
  the colour picker's encoders (ColorEditBar: x, y, brightness, colour space), not attribute encoders".
- The control module's console backend writes attribute values (`Attribute "<name>" At +/- n`). The picker's
  encoders are not attributes, so there is no command form to send yet.
- The surface refuses every strip and rotary event with that reason, and its lights show the slots as unavailable.

## What has to be found out first (console side, GrandMA3MCP)

1. How the `ColorEditBar`'s encoders are addressed: names and order, what each one edits, their current values, and
   whether they change with the picker's mode (CIE / RGB / fader / swatch book; the screenshot showed CIE with x, y
   and B, and a colour-space selector).
2. Which command or API applies a relative change to one of them, as `Attribute … At +n` does for attributes:
   candidates are a picker-specific keyword or property, encoder emulation, or computing the resulting RGB/CMY
   attribute values from x, y, brightness and writing those.
3. How a change is read back for the bars and the binding generation.

## Shape of the change once that is known

- feedback: a `colorPicker` context (mode, encoders, values) next to the attribute slots, with its own generation.
- control: a backend path for picker encoders (relative, fine), qualified live, refusing what is not qualified.
- surface: rotaries and strips target picker encoders while the picker is open; the lights show their values.
- A probe script and a live record, as for KB-19.

Handoff note for a new session: start from this file, the M-Touch record and `plugin/gma3_mcp_feedback.lua`'s
`encoderUi`; probe the `ColorEditBar` tree on display 1 with the picker open (the hardware pass found it under
`EncoderBarContainer.EncoderBarGrid.EncoderBar`, with `Encoder1`–`Encoder4` `EncoderControl`s and screen encoders).
