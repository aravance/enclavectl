//! `enclavectl` — control an Enclave CineHome Smart Center over Bluetooth.
//!
//! The unit does not answer HDMI-CEC while in standby, so power has to come over
//! Bluetooth. See `src/proto.rs` for the wire format.

mod link;
mod proto;

use std::process::ExitCode;
use std::time::{Duration, Instant};

use link::Link;
use proto::{Cmd, Status, cec_mode_value, input_value};

const DEFAULT_ADDRESS: &str = "00:07:80:C4:46:09";
const DEFAULT_CHANNEL: u8 = 1;

const CONNECT_ATTEMPTS: u32 = 3;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
/// Right after a resume the Bluetooth stack may not be ready yet.
const RETRY_PAUSE: Duration = Duration::from_secs(2);
/// A powered-down unit simply does not answer, so the pre-check stays short.
const PRECHECK_WINDOW: Duration = Duration::from_millis(1500);
const PRECHECK_TRIES: u32 = 3;
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(25);
/// The top of the official app's volume slider.
const MAX_VOLUME: u8 = 60;

const USAGE: &str = "\
enclavectl — control an Enclave CineHome over Bluetooth

usage: enclavectl [options] <command>

commands:
  status                 report the current state (exit 2 if it is off)
  on                     power on, if not on already
  off                    power off
  mute <on|off>
  volume <0-60>
  input <hdmi1|hdmi2|hdmi3|optical|arc|bt>
  cec <off|on|arc>
  raw <cmd> <value>      send any command number and value (levels may be negative)

options:
  -a, --address <mac>    default: $ENCLAVE_ADDR, else 00:07:80:C4:46:09
  -c, --channel <n>      RFCOMM channel (default 1)
      --json             machine-readable status
  -q, --quiet            suppress progress messages on stderr
  -h, --help

exit codes: 0 ok · 1 could not reach the device · 2 off / not confirmed · 3 bad usage
";

struct Opts {
    address: String,
    channel: u8,
    json: bool,
    quiet: bool,
    args: Vec<String>,
}

fn parse_opts() -> Result<Opts, String> {
    let mut opts = Opts {
        address: std::env::var("ENCLAVE_ADDR").unwrap_or_else(|_| DEFAULT_ADDRESS.to_string()),
        channel: DEFAULT_CHANNEL,
        json: false,
        quiet: false,
        args: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--json" => opts.json = true,
            "-q" | "--quiet" => opts.quiet = true,
            "-a" | "--address" => {
                opts.address = it.next().ok_or("--address needs a value")?;
            }
            "-c" | "--channel" => {
                let v = it.next().ok_or("--channel needs a value")?;
                opts.channel = v.parse().map_err(|_| format!("bad channel: {v}"))?;
            }
            // A negative number is a level value for `raw`, not an option.
            other if other.starts_with('-') && other.parse::<i16>().is_err() => {
                return Err(format!("unknown option: {other}"));
            }
            other => opts.args.push(other.to_string()),
        }
    }
    Ok(opts)
}

fn main() -> ExitCode {
    let opts = match parse_opts() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("enclavectl: {e}\n");
            eprint!("{USAGE}");
            return ExitCode::from(3);
        }
    };
    if opts.args.is_empty() {
        eprint!("{USAGE}");
        return ExitCode::from(3);
    }

    match run(&opts) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("enclavectl: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(opts: &Opts) -> Result<ExitCode, String> {
    let address = link::parse_address(&opts.address)
        .ok_or_else(|| format!("bad address: {}", opts.address))?;

    let mut link = open(address, opts.channel, opts)?;
    note(
        opts,
        &format!("connected to {}, firmware {}", opts.address, link.firmware),
    );

    let cmd = opts.args[0].as_str();
    let arg = opts.args.get(1).map(String::as_str);

    match cmd {
        "status" => match current(&mut link)? {
            Some(status) => {
                if opts.json {
                    println!("{}", status.to_json());
                } else {
                    println!("{status}");
                }
                Ok(ExitCode::SUCCESS)
            }
            None => {
                if opts.json {
                    println!(r#"{{"power":0,"reachable":true,"standby":true}}"#);
                } else {
                    println!("power         off (no status reply, the standby signature)");
                }
                Ok(ExitCode::from(2))
            }
        },
        "on" => {
            if current(&mut link)?.is_some_and(|s| s.is_on()) {
                note(opts, "already on, nothing to do");
                return Ok(ExitCode::SUCCESS);
            }
            note(opts, "off, sending power on");
            set(&mut link, Cmd::Power, 1)?;
            Ok(confirm(&mut link, opts, true)?)
        }
        "off" => {
            set(&mut link, Cmd::Power, 0)?;
            note(opts, "sent power off");
            Ok(confirm(&mut link, opts, false)?)
        }
        "mute" => {
            let value = match arg {
                Some("on") | Some("1") => 1,
                Some("off") | Some("0") => 0,
                _ => return Err("mute needs on or off".into()),
            };
            set(&mut link, Cmd::Mute, value)?;
            report(&mut link, opts)
        }
        "volume" => {
            let value = numeric(arg, "volume")?;
            if value > MAX_VOLUME {
                return Err(format!("bad volume: {value} (expected 0-{MAX_VOLUME})"));
            }
            set(&mut link, Cmd::Volume, value)?;
            report(&mut link, opts)
        }
        "input" => {
            let name = arg.ok_or("input needs a source name")?;
            let value = input_value(name).ok_or_else(|| format!("unknown input: {name}"))?;
            set(&mut link, Cmd::Input, value)?;
            report(&mut link, opts)
        }
        "cec" => {
            let name = arg.ok_or("cec needs a mode name")?;
            let value = cec_mode_value(name).ok_or_else(|| format!("unknown cec mode: {name}"))?;
            set(&mut link, Cmd::CecMode, value)?;
            report(&mut link, opts)
        }
        "raw" => {
            let number = numeric(arg, "cmd")?;
            let raw = opts.args.get(2).ok_or("raw needs a value")?;
            let value = match raw.parse::<i16>() {
                // Negative levels go out as their two's complement.
                Ok(v @ -128..=255) => v as u8,
                _ => return Err(format!("bad value: {raw} (expected -128 to 255)")),
            };
            link.send_raw(number, value).map_err(|e| e.to_string())?;
            note(opts, &format!("sent raw cmd {number} value {raw}"));
            report(&mut link, opts)
        }
        other => Err(format!("unknown command: {other}")),
    }
}

fn open(address: [u8; 6], channel: u8, opts: &Opts) -> Result<Link, String> {
    let mut last = String::from("unknown error");
    for attempt in 1..=CONNECT_ATTEMPTS {
        match Link::open(address, channel, CONNECT_TIMEOUT) {
            Ok(link) => return Ok(link),
            Err(e) => {
                last = e.to_string();
                note(opts, &format!("connect attempt {attempt} failed: {last}"));
                if attempt < CONNECT_ATTEMPTS {
                    std::thread::sleep(RETRY_PAUSE);
                }
            }
        }
    }
    Err(format!("cannot reach {}: {last}", opts.address))
}

/// Current state, or `None` if it never answered (i.e. standby).
fn current(link: &mut Link) -> Result<Option<Status>, String> {
    for _ in 0..PRECHECK_TRIES {
        match link.status(PRECHECK_WINDOW).map_err(|e| e.to_string())? {
            Some(status) => return Ok(Some(status)),
            None => continue,
        }
    }
    Ok(None)
}

fn set(link: &mut Link, cmd: Cmd, value: u8) -> Result<(), String> {
    link.send(cmd, value).map_err(|e| e.to_string())
}

/// Polls until the power state matches `want`, or the confirm window runs out.
fn confirm(link: &mut Link, opts: &Opts, want_on: bool) -> Result<ExitCode, String> {
    let deadline = Instant::now() + CONFIRM_TIMEOUT;
    while Instant::now() < deadline {
        let status = link
            .status(Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        let settled = match (&status, want_on) {
            (Some(s), true) => s.is_on(),
            (Some(s), false) => !s.is_on(),
            // Silence means standby, which confirms "off" and never confirms "on".
            (None, false) => true,
            (None, true) => false,
        };
        if settled {
            note(
                opts,
                if want_on {
                    "confirmed on"
                } else {
                    "confirmed off"
                },
            );
            return Ok(ExitCode::SUCCESS);
        }
    }
    note(opts, "command sent but the device did not confirm it");
    Ok(ExitCode::from(2))
}

fn report(link: &mut Link, opts: &Opts) -> Result<ExitCode, String> {
    match current(link)? {
        Some(status) => {
            if opts.json {
                println!("{}", status.to_json());
            } else {
                println!("{status}");
            }
            Ok(ExitCode::SUCCESS)
        }
        None => {
            note(
                opts,
                "no status reply after the command (is it in standby?)",
            );
            Ok(ExitCode::from(2))
        }
    }
}

fn numeric(arg: Option<&str>, what: &str) -> Result<u8, String> {
    let raw = arg.ok_or_else(|| format!("{what} needs a value"))?;
    raw.parse()
        .map_err(|_| format!("bad {what}: {raw} (expected 0-255)"))
}

fn note(opts: &Opts, msg: &str) {
    if !opts.quiet {
        eprintln!("enclavectl: {msg}");
    }
}
