# enclavectl

Command-line control of an Enclave CineHome (Smart Center) over Bluetooth, for Linux.
Useful for power in particular: the unit ignores HDMI-CEC while in standby.

```
enclavectl status          # exit 2 when it is off
enclavectl on              # no-op if already on
enclavectl off
enclavectl mute on|off
enclavectl volume 27
enclavectl input hdmi1|hdmi2|hdmi3|optical|arc|bt
enclavectl cec off|on|arc
enclavectl raw 153 1       # any command number and value (here: Pro Logic auto)
```

Options: `-a/--address <mac>` (or `$ENCLAVE_ADDR`), `-c/--channel` (default 1), `--json`,
`-q/--quiet`.
Exit codes: `0` ok · `1` unreachable · `2` off or unconfirmed · `3` bad usage.

## Build

```sh
cargo build --release
install -m 755 target/release/enclavectl /usr/local/bin/
install -Dm 644 completions/enclavectl.bash /usr/local/share/bash-completion/completions/enclavectl
install -Dm 644 completions/_enclavectl /usr/local/share/zsh/site-functions/_enclavectl
```

No root or group membership is needed at runtime.

## Do not pair the device

The tool connects unbonded. Pairing also makes the Enclave an audio sink, and the host may
then route sound to it over Bluetooth instead of HDMI.

## Protocol

Bluetooth Classic, RFCOMM to the Serial Port Profile. Handshake:

```
device -> "W\r\n"
host   -> "EBRC\n"
device -> "V<firmware>\r\n"
```

Commands are four bytes: `[cmd, value + 0x80, (cmd + value + 0x80) & 0xFF, '\n']`

```
144 power (0/1)            145 mute (0/1)          146 master volume (0-60)
147 woofer (-10..10)       148 center (-6..6)      149 rear (-6..6)
150 rear balance (-6..6)   151/152 rear delay L/R (0-10)
153 Pro Logic (0 off, 1 auto, 2 movie, 3 music)
154 dynamic range (0 standard, 1 minimum, 2 maximum)
155 CEC mode (0 off, 1 on, 2 ARC)
156 input (0-2 = HDMI 1-3, 3 optical, 4 aux/ARC, 5 Bluetooth)
157 test noise (0/1)       158 reset speakers (1) 159 status request
```

Levels are signed, so -3 is sent as `0x7d`. The ranges are the official app's sliders.

A status reply is 15 payload bytes, a checksum byte (their sum, truncated) and CRLF.
Bytes 0–12 are `value + 0x80` for commands 144–156, with two twists: a CEC value of 3
means ARC mode with the link up (a reported state, not a mode you can send), and bit
`0x10` of the input byte is set while test noise plays. Byte 13 is the stream type in its low two bits (0 PCM, 1 Dolby Digital Plus,
2 Dolby Digital, 3 DTS). Byte 14 flags detected speakers, bits 0–6 = L, R, LS, RS, C,
Sub1, Sub2. In standby the unit sends no reply at all, so silence means off.

## License

Apache License 2.0; see `LICENSE.txt`.
