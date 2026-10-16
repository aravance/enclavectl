//! Wire format of the Enclave CineHome Smart Center's remote-control protocol.
//!
//! Every command is four bytes: `[cmd, value + 0x80, (cmd + value + 0x80) & 0xFF, b'\n']`.
//! Status replies are 15 payload bytes, then a checksum (sum of the payload, truncated) and
//! CRLF. The first thirteen bytes are `value + 0x80` in command order from [`Cmd::Power`]
//! to [`Cmd::Input`]; the last two report other state, see [`Status::decode`].
//!
//! Level values (woofer, center, rear, rear balance) are signed: -3 goes out as `0x7d`.

/// Command numbers. The status payload carries `Power` through `Input` in order.
///
/// The variants we never send are kept deliberately: together they document the device's
/// whole command set, and `enclavectl raw` can reach any of them by number.
#[allow(dead_code)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Cmd {
    Power = 144,
    Mute = 145,
    Volume = 146,
    Woofer = 147,
    Center = 148,
    Rear = 149,
    RearBalance = 150,
    RearDelayL = 151,
    RearDelayR = 152,
    ProLogic = 153,
    DynamicRange = 154,
    CecMode = 155,
    Input = 156,
    TestNoise = 157,
    Reset = 158,
    Status = 159,
}

/// Length of a status reply: 15 payload bytes + checksum + CRLF.
pub const STATUS_LEN: usize = 18;
const PAYLOAD_LEN: usize = 15;

/// A value of 0 is encoded as 0x80, so that is what "off" looks like in `power`.
const BIAS: u8 = 0x80;
/// Set in the input byte of a status reply while the test noise sequence runs.
const TEST_NOISE_FLAG: u8 = 0x10;
/// CEC byte of a status reply when the mode is ARC and the ARC link is up. Never sent.
const CEC_ARC_ACTIVE: u8 = 3;
/// Bit order of the detected-speakers byte in a status reply.
const SPEAKERS: [&str; 7] = ["L", "R", "LS", "RS", "C", "Sub1", "Sub2"];

pub fn frame(cmd: Cmd, value: u8) -> [u8; 4] {
    frame_raw(cmd as u8, value)
}

/// Frames any command number, including ones outside [`Cmd`], so undocumented settings
/// can be probed without pretending an arbitrary byte is a valid enum variant.
pub fn frame_raw(cmd: u8, value: u8) -> [u8; 4] {
    let encoded = value.wrapping_add(BIAS);
    [cmd, encoded, cmd.wrapping_add(encoded), b'\n']
}

/// Which source the unit is listening to. Values outside the known set are kept as-is.
pub fn input_name(value: u8) -> &'static str {
    match value {
        0 => "HDMI 1",
        1 => "HDMI 2",
        2 => "HDMI 3",
        3 => "optical",
        4 => "aux/ARC",
        5 => "Bluetooth",
        _ => "unknown",
    }
}

/// Parses an input name back to its protocol value.
pub fn input_value(name: &str) -> Option<u8> {
    match name.to_ascii_lowercase().as_str() {
        "hdmi1" => Some(0),
        "hdmi2" => Some(1),
        "hdmi3" => Some(2),
        "optical" => Some(3),
        "arc" | "aux" => Some(4),
        "bt" | "bluetooth" => Some(5),
        _ => None,
    }
}

/// How the unit takes part in HDMI-CEC. Values outside the known set are kept as-is.
pub fn cec_mode_name(value: u8) -> &'static str {
    match value {
        0 => "off",
        1 => "on",
        2 => "ARC",
        _ => "unknown",
    }
}

/// Parses a CEC mode name back to its protocol value.
pub fn cec_mode_value(name: &str) -> Option<u8> {
    match name.to_ascii_lowercase().as_str() {
        "off" => Some(0),
        "on" => Some(1),
        "arc" => Some(2),
        _ => None,
    }
}

pub fn pro_logic_name(value: u8) -> &'static str {
    match value {
        0 => "off",
        1 => "auto",
        2 => "movie",
        3 => "music",
        _ => "unknown",
    }
}

pub fn dynamic_range_name(value: u8) -> &'static str {
    match value {
        0 => "standard",
        1 => "minimum",
        2 => "maximum",
        _ => "unknown",
    }
}

/// Format of the incoming audio stream, as reported in a status reply.
pub fn stream_type_name(value: u8) -> &'static str {
    match value {
        0 => "PCM",
        1 => "Dolby Digital Plus",
        2 => "Dolby Digital",
        3 => "DTS",
        _ => "unknown",
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub power: u8,
    pub mute: u8,
    pub volume: u8,
    pub woofer: i8,
    pub center: i8,
    pub rear: i8,
    pub rear_balance: i8,
    pub rear_delay_l: u8,
    pub rear_delay_r: u8,
    pub pro_logic: u8,
    pub dynamic_range: u8,
    pub cec_mode: u8,
    /// The CEC mode is ARC and the ARC link is actually up.
    /// Output only: there is no command value that sets it.
    pub arc_active: bool,
    pub input: u8,
    pub test_noise: bool,
    pub stream_type: u8,
    /// Detected speakers, one bit each in [`SPEAKERS`] order.
    pub speakers: u8,
}

impl Status {
    /// Decodes a status reply, returning `None` if it is short or the checksum disagrees.
    ///
    /// Bytes 0–12 are `value + 0x80` in command order, except that a CEC value of 3 means
    /// ARC mode with the link up, and the input byte also carries the test-noise flag.
    /// Byte 13 is the stream type in its low two bits and byte 14 the detected speakers.
    ///
    /// Note that a powered-down unit does not reply at all, so absence of a status —
    /// rather than a status saying "off" — is the usual way to learn it is in standby.
    pub fn decode(buf: &[u8]) -> Option<Self> {
        if buf.len() < STATUS_LEN {
            return None;
        }
        let payload = &buf[..PAYLOAD_LEN];
        let sum = payload.iter().fold(0u8, |acc, b| acc.wrapping_add(*b));
        if sum != buf[PAYLOAD_LEN] {
            return None;
        }
        let v = |i: usize| payload[i].wrapping_sub(BIAS);
        let level = |i: usize| v(i) as i8;
        let cec = v(11);
        Some(Self {
            power: v(0),
            mute: v(1),
            volume: v(2),
            woofer: level(3),
            center: level(4),
            rear: level(5),
            rear_balance: level(6),
            rear_delay_l: v(7),
            rear_delay_r: v(8),
            pro_logic: v(9),
            dynamic_range: v(10),
            cec_mode: if cec == CEC_ARC_ACTIVE { 2 } else { cec },
            arc_active: cec == CEC_ARC_ACTIVE,
            input: (payload[12] & !TEST_NOISE_FLAG).wrapping_sub(BIAS),
            test_noise: payload[12] & TEST_NOISE_FLAG != 0,
            stream_type: payload[13] & 0x03,
            speakers: payload[14] & 0x7f,
        })
    }

    pub fn is_on(&self) -> bool {
        self.power != 0
    }

    /// Names of the detected speakers, e.g. `["L", "R", "C"]`.
    pub fn speaker_names(&self) -> impl Iterator<Item = &'static str> {
        let speakers = self.speakers;
        SPEAKERS
            .iter()
            .enumerate()
            .filter(move |(bit, _)| speakers & (1 << bit) != 0)
            .map(|(_, name)| *name)
    }

    pub fn to_json(self) -> String {
        let speakers: Vec<String> = self.speaker_names().map(|n| format!(r#""{n}""#)).collect();
        format!(
            concat!(
                r#"{{"power":{},"mute":{},"volume":{},"woofer":{},"center":{},"rear":{},"#,
                r#""rear_balance":{},"rear_delay_l":{},"rear_delay_r":{},"#,
                r#""pro_logic":{},"pro_logic_name":"{}","#,
                r#""dynamic_range":{},"dynamic_range_name":"{}","#,
                r#""cec_mode":{},"cec_mode_name":"{}","arc_active":{},"#,
                r#""input":{},"input_name":"{}","test_noise":{},"#,
                r#""stream_type":{},"stream_type_name":"{}","speakers":[{}]}}"#
            ),
            self.power,
            self.mute,
            self.volume,
            self.woofer,
            self.center,
            self.rear,
            self.rear_balance,
            self.rear_delay_l,
            self.rear_delay_r,
            self.pro_logic,
            pro_logic_name(self.pro_logic),
            self.dynamic_range,
            dynamic_range_name(self.dynamic_range),
            self.cec_mode,
            cec_mode_name(self.cec_mode),
            self.arc_active,
            self.input,
            input_name(self.input),
            self.test_noise,
            self.stream_type,
            stream_type_name(self.stream_type),
            speakers.join(","),
        )
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "power         {}",
            if self.is_on() { "on" } else { "off" }
        )?;
        writeln!(f, "mute          {}", self.mute)?;
        writeln!(f, "volume        {}", self.volume)?;
        writeln!(f, "woofer        {}", self.woofer)?;
        writeln!(f, "center        {}", self.center)?;
        writeln!(f, "rear          {}", self.rear)?;
        writeln!(f, "rear balance  {}", self.rear_balance)?;
        writeln!(
            f,
            "rear delay    {} / {}",
            self.rear_delay_l, self.rear_delay_r
        )?;
        writeln!(
            f,
            "pro logic     {} ({})",
            self.pro_logic,
            pro_logic_name(self.pro_logic)
        )?;
        writeln!(
            f,
            "dynamic range {} ({})",
            self.dynamic_range,
            dynamic_range_name(self.dynamic_range)
        )?;
        writeln!(
            f,
            "cec mode      {} ({}{})",
            self.cec_mode,
            cec_mode_name(self.cec_mode),
            if self.arc_active { ", active" } else { "" }
        )?;
        writeln!(
            f,
            "input         {} ({})",
            self.input,
            input_name(self.input)
        )?;
        writeln!(f, "stream        {}", stream_type_name(self.stream_type))?;
        writeln!(
            f,
            "speakers      {}",
            self.speaker_names().collect::<Vec<_>>().join(" ")
        )?;
        write!(
            f,
            "test noise    {}",
            if self.test_noise { "on" } else { "off" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A status reply from a unit that is on.
    const CAPTURED: [u8; STATUS_LEN] = [
        0x81, 0x81, 0x9b, 0x80, 0x83, 0x81, 0x80, 0x80, 0x80, 0x81, 0x80, 0x82, 0x84, 0x80, 0xbd,
        0xe5, 0x0d, 0x0a,
    ];

    #[test]
    fn decodes_captured_status() {
        let s = Status::decode(&CAPTURED).expect("checksum should verify");
        assert!(s.is_on());
        assert_eq!(s.volume, 27);
        assert_eq!(s.mute, 1);
        assert_eq!(s.input, 4);
        assert_eq!(input_name(s.input), "aux/ARC");
        assert_eq!(s.cec_mode, 2);
        assert_eq!(cec_mode_name(s.cec_mode), "ARC");
        assert!(!s.arc_active);
        assert_eq!(s.center, 3);
        assert_eq!(pro_logic_name(s.pro_logic), "auto");
        assert!(!s.test_noise);
        assert_eq!(stream_type_name(s.stream_type), "PCM");
        assert_eq!(
            s.speaker_names().collect::<Vec<_>>(),
            ["L", "LS", "RS", "C", "Sub1"]
        );
    }

    /// `CAPTURED` with the given payload bytes replaced and the checksum redone.
    fn captured_with(changes: &[(usize, u8)]) -> [u8; STATUS_LEN] {
        let mut buf = CAPTURED;
        for &(i, b) in changes {
            buf[i] = b;
        }
        buf[PAYLOAD_LEN] = buf[..PAYLOAD_LEN]
            .iter()
            .fold(0u8, |acc, b| acc.wrapping_add(*b));
        buf
    }

    #[test]
    fn levels_are_signed() {
        let s = Status::decode(&captured_with(&[(3, 0x76), (4, 0x7d)])).unwrap();
        assert_eq!(s.woofer, -10);
        assert_eq!(s.center, -3);
        assert_eq!(
            frame_raw(Cmd::Center as u8, -3i8 as u8),
            [0x94, 0x7d, 0x11, b'\n']
        );
    }

    #[test]
    fn test_noise_flag_leaves_input_intact() {
        let s = Status::decode(&captured_with(&[(12, 0x84 | 0x10)])).unwrap();
        assert!(s.test_noise);
        assert_eq!(s.input, 4);
    }

    #[test]
    fn cec_three_is_arc_with_the_link_up() {
        let s = Status::decode(&captured_with(&[(11, 0x83)])).unwrap();
        assert_eq!(s.cec_mode, 2);
        assert!(s.arc_active);
    }

    #[test]
    fn rejects_bad_checksum() {
        let mut bad = CAPTURED;
        bad[15] ^= 0xff;
        assert!(Status::decode(&bad).is_none());
    }

    #[test]
    fn rejects_short_reply() {
        assert!(Status::decode(&CAPTURED[..10]).is_none());
    }

    #[test]
    fn power_off_payload_reads_as_off() {
        // Every field zero: payload bytes are all 0x80, checksum 15 * 0x80 = 0x80.
        let mut buf = [0x80u8; STATUS_LEN];
        buf[15] = 0x80;
        buf[16] = b'\r';
        buf[17] = b'\n';
        let s = Status::decode(&buf).expect("checksum should verify");
        assert!(!s.is_on());
    }

    #[test]
    fn frames_match_the_documented_examples() {
        // Status request: 159 + (0 + 0x80) = 0x9f 0x80 0x1f '\n'
        assert_eq!(frame(Cmd::Status, 0), [0x9f, 0x80, 0x1f, b'\n']);
        // Power on: 144 + (1 + 0x80) = 0x90 0x81 0x11 '\n'
        assert_eq!(frame(Cmd::Power, 1), [0x90, 0x81, 0x11, b'\n']);
        // Power off: 144 + (0 + 0x80) = 0x90 0x80 0x10 '\n'
        assert_eq!(frame(Cmd::Power, 0), [0x90, 0x80, 0x10, b'\n']);
    }

    #[test]
    fn input_names_round_trip() {
        for (name, value) in [
            ("hdmi1", 0),
            ("hdmi3", 2),
            ("optical", 3),
            ("arc", 4),
            ("bt", 5),
        ] {
            assert_eq!(input_value(name), Some(value));
        }
        assert_eq!(input_value("nope"), None);
    }

    #[test]
    fn cec_mode_names_round_trip() {
        for (name, value) in [("off", 0), ("on", 1), ("arc", 2)] {
            assert_eq!(cec_mode_value(name), Some(value));
        }
        assert_eq!(cec_mode_value("nope"), None);
    }
}
