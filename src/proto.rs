//! Wire format of the Enclave CineHome Smart Center's remote-control protocol.
//!
//! Reverse-engineered from the official app (`com.enclaveaudio.app` 1.0.33) against
//! firmware V152; see the Android implementation in `/build/enclave-android`.
//!
//! Every command is four bytes: `[cmd, value + 0x80, (cmd + value + 0x80) & 0xFF, b'\n']`.
//! Status replies are 15 payload bytes, each `value + 0x80` in command order starting at
//! [`Cmd::Power`], then a checksum (sum of the payload, truncated) and CRLF.

/// Command numbers. The status payload carries these in order, starting at `Power`.
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub power: u8,
    pub mute: u8,
    pub volume: u8,
    pub woofer: u8,
    pub center: u8,
    pub rear: u8,
    pub rear_balance: u8,
    pub rear_delay_l: u8,
    pub rear_delay_r: u8,
    pub pro_logic: u8,
    pub dynamic_range: u8,
    pub cec_mode: u8,
    pub input: u8,
    pub test_noise: u8,
    pub reset: u8,
}

impl Status {
    /// Decodes a status reply, returning `None` if it is short or the checksum disagrees.
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
        Some(Self {
            power: v(0),
            mute: v(1),
            volume: v(2),
            woofer: v(3),
            center: v(4),
            rear: v(5),
            rear_balance: v(6),
            rear_delay_l: v(7),
            rear_delay_r: v(8),
            pro_logic: v(9),
            dynamic_range: v(10),
            cec_mode: v(11),
            input: v(12),
            test_noise: v(13),
            reset: v(14),
        })
    }

    pub fn is_on(&self) -> bool {
        self.power != 0
    }

    pub fn to_json(self) -> String {
        format!(
            concat!(
                r#"{{"power":{},"mute":{},"volume":{},"woofer":{},"center":{},"rear":{},"#,
                r#""rear_balance":{},"rear_delay_l":{},"rear_delay_r":{},"pro_logic":{},"#,
                r#""dynamic_range":{},"cec_mode":{},"input":{},"input_name":"{}","#,
                r#""test_noise":{},"reset":{}}}"#
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
            self.dynamic_range,
            self.cec_mode,
            self.input,
            input_name(self.input),
            self.test_noise,
            self.reset,
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
        writeln!(f, "pro logic     {}", self.pro_logic)?;
        writeln!(f, "dynamic range {}", self.dynamic_range)?;
        writeln!(f, "cec mode      {}", self.cec_mode)?;
        writeln!(
            f,
            "input         {} ({})",
            self.input,
            input_name(self.input)
        )?;
        writeln!(f, "test noise    {}", self.test_noise)?;
        write!(f, "reset         {}", self.reset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real status reply captured from firmware V152 while the unit was on.
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
        assert_eq!(s.center, 3);
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
}
