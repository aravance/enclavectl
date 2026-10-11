# enclavectl

Command-line control of an Enclave CineHome (Smart Center) over Bluetooth, for Linux.

The unit **does not answer HDMI-CEC while in standby** — every CEC message to its logical
address comes back `Not Acknowledged`, because its CEC block isn't powered. It does answer
CEC once already on, which makes the failure easy to misdiagnose. So power has to come over
Bluetooth.

```
enclavectl status          # exit 2 when it is off
enclavectl on              # no-op if already on
enclavectl off
enclavectl mute on|off
enclavectl volume 27
enclavectl input hdmi1|hdmi2|hdmi3|optical|arc|bt
enclavectl raw 155 2       # any command number, for undocumented settings
```

Options: `-a/--address` (default `$ENCLAVE_ADDR`, else the address compiled in),
`-c/--channel` (default 1), `--json`, `-q/--quiet`.
Exit codes: `0` ok · `1` unreachable · `2` off or unconfirmed · `3` bad usage.

## Build

```sh
cargo build --release
install -m 755 target/release/enclavectl /usr/local/bin/
```

One dependency (`libc`): Rust's std has no Bluetooth sockets, so the RFCOMM socket is
opened directly. All the `unsafe` is confined to `src/link.rs`.

## Do not pair the device

The Enclave asks for no pairing, and BlueZ will connect to it unbonded — which is what this
tool relies on. Pairing it also makes it an audio sink, and the host then routes sound to it
over Bluetooth instead of HDMI. On Android, bonding is mandatory before RFCOMM opens, which
is why the companion project needs a guard to tear the audio link back down; on Linux the
problem simply doesn't arise.

An unprivileged user can open the socket, so this needs no root and no group membership.

## Protocol

Reverse-engineered from the official app (`com.enclaveaudio.app` 1.0.33), confirmed against
firmware V152. The Android implementation is in `/build/enclave-android`
(`app/src/main/java/app/enclavepower/EnclaveClient.kt`).

Transport is Bluetooth Classic, RFCOMM to the Serial Port Profile
(`00001101-0000-1000-8000-00805F9B34FB`), channel 1 on the unit tested.

Handshake:

```
device -> "W\r\n"
host   -> "EBRC\n"
device -> "V<firmware>\r\n"
```

Commands are four bytes, where the value is biased by `0x80` and the third byte is a
truncated sum:

```
[cmd, value + 0x80, (cmd + value + 0x80) & 0xFF, '\n']

144 power (0/1)     145 mute (0/1)      146 master volume
147 woofer          148 center          149 rear
150 rear balance    151/152 rear delay L/R
153 Pro Logic       154 dynamic range   155 CEC mode
156 input (0-2 = HDMI 1-3, 3 optical, 4 aux/ARC, 5 Bluetooth)
157 test noise      158 reset speakers  159 status request
```

A status reply is 15 payload bytes — each `value + 0x80`, in command order starting at
power — then a checksum (the payload summed and truncated) and CRLF. `payload[0] == 0x80`
means off.

**In standby the unit does not reply at all.** Silence, not a status saying "off", is how you
learn it is asleep; `src/proto.rs` carries a real captured reply as a test fixture.

## Unknowns

- The `CEC mode` setting (155) reads 2 on the tested unit and its enum is undocumented. It
  cannot make the unit answer CEC in standby, since nothing is powered to acknowledge, but
  it may affect behaviour while on.
- Valid ranges for the level controls aren't established; the protocol accepts 0–127.
- `reset` (byte 14 of the status payload) read 61 on a powered unit, which doesn't look like
  a flag. That byte may mean something else.

## License

Apache License 2.0; see `LICENSE.txt`.
