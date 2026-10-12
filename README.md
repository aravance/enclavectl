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
enclavectl raw 153 1       # any command number and value (here: Pro Logic on)
```

Options: `-a/--address <mac>` (or `$ENCLAVE_ADDR`), `-c/--channel` (default 1), `--json`,
`-q/--quiet`.
Exit codes: `0` ok · `1` unreachable · `2` off or unconfirmed · `3` bad usage.

## Build

```sh
cargo build --release
install -m 755 target/release/enclavectl /usr/local/bin/
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
144 power (0/1)     145 mute (0/1)      146 master volume
147 woofer          148 center          149 rear
150 rear balance    151/152 rear delay L/R
153 Pro Logic       154 dynamic range
155 CEC mode (0 off, 1 on, 2 ARC)
156 input (0-2 = HDMI 1-3, 3 optical, 4 aux/ARC, 5 Bluetooth)
157 test noise      158 reset speakers  159 status request
```

A status reply is 15 payload bytes (each `value + 0x80`, in command order from 144), a
checksum byte and CRLF. In standby the unit sends no reply at all, so silence means off.

Valid ranges for the level controls aren't established; the protocol accepts 0–127.

## License

Apache License 2.0; see `LICENSE.txt`.
