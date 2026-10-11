//! Bluetooth Classic RFCOMM link to the Enclave, and the handshake it expects.
//!
//! The unit asks for no pairing, so BlueZ connects to it unbonded. That matters: pairing
//! it would also make it an audio sink, and the host would start routing sound there
//! instead of over HDMI. Do not pair it.
//!
//! All the `unsafe` in this crate lives here. Rust's std has no Bluetooth sockets, so the
//! socket is opened directly with `AF_BLUETOOTH` / `BTPROTO_RFCOMM` and a `sockaddr_rc`.

use std::io;
use std::mem;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::{Duration, Instant};

use crate::proto::{Cmd, STATUS_LEN, Status, frame, frame_raw};

const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_RFCOMM: libc::c_int = 3;

/// `struct sockaddr_rc` from the kernel's RFCOMM headers. Deliberately `repr(C)` rather
/// than packed: the kernel compares against `sizeof(struct sockaddr_rc)`, which includes
/// the tail padding after `rc_channel`.
#[repr(C)]
#[derive(Copy, Clone)]
struct SockAddrRc {
    rc_family: libc::sa_family_t,
    rc_bdaddr: [u8; 6],
    rc_channel: u8,
}

/// Parses `00:07:80:C4:46:09` into the little-endian byte order the kernel wants.
pub fn parse_address(s: &str) -> Option<[u8; 6]> {
    let mut out = [0u8; 6];
    let mut seen = 0;
    for (i, part) in s.split(':').enumerate() {
        if i >= 6 || part.len() != 2 {
            return None;
        }
        out[5 - i] = u8::from_str_radix(part, 16).ok()?;
        seen += 1;
    }
    (seen == 6).then_some(out)
}

pub struct Link {
    fd: OwnedFd,
    pub firmware: String,
}

impl Link {
    /// Connects, then completes the handshake: device sends `W`, we answer `EBRC`, device
    /// replies with its firmware version.
    pub fn open(address: [u8; 6], channel: u8, connect_timeout: Duration) -> io::Result<Self> {
        let fd = connect(address, channel, connect_timeout)?;
        // A short socket timeout keeps reads non-blocking enough to poll against a deadline.
        set_recv_timeout(&fd, Duration::from_millis(300))?;
        let mut link = Self {
            fd,
            firmware: String::new(),
        };

        let greeting = link.read_message(Duration::from_secs(5))?;
        match greeting {
            Some(msg) if msg.first() == Some(&b'W') => {}
            other => {
                return Err(io::Error::other(format!(
                    "no greeting from device (got {other:?})"
                )));
            }
        }
        link.write_all(b"EBRC\n")?;
        let version = link.read_message(Duration::from_secs(5))?;
        match version {
            Some(msg) if msg.first() == Some(&b'V') => {
                link.firmware = String::from_utf8_lossy(&msg[1..]).trim().to_string();
            }
            other => {
                return Err(io::Error::other(format!(
                    "no version from device (got {other:?})"
                )));
            }
        }
        Ok(link)
    }

    pub fn send(&self, cmd: Cmd, value: u8) -> io::Result<()> {
        self.write_all(&frame(cmd, value))
    }

    /// Sends any command number, for settings the protocol notes don't cover.
    pub fn send_raw(&self, cmd: u8, value: u8) -> io::Result<()> {
        self.write_all(&frame_raw(cmd, value))
    }

    /// Asks for status and waits up to `window` for a well-formed reply.
    ///
    /// `Ok(None)` means it did not answer, which for this device is how standby looks.
    pub fn status(&mut self, window: Duration) -> io::Result<Option<Status>> {
        self.send(Cmd::Status, 0)?;
        let deadline = Instant::now() + window;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.read_message(remaining)? {
                Some(msg) if msg.len() >= STATUS_LEN => {
                    if let Some(status) = Status::decode(&msg) {
                        return Ok(Some(status));
                    }
                }
                Some(_) => continue, // some other message; keep waiting
                None => return Ok(None),
            }
        }
        Ok(None)
    }

    /// Reads one CRLF-terminated message, or `None` once `timeout` elapses.
    fn read_message(&self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        let mut buf = Vec::with_capacity(64);
        let mut chunk = [0u8; 64];
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let n = unsafe {
                libc::read(
                    self.fd.as_raw_fd(),
                    chunk.as_mut_ptr().cast::<libc::c_void>(),
                    chunk.len(),
                )
            };
            if n > 0 {
                buf.extend_from_slice(&chunk[..n as usize]);
                if buf.len() > 2 && buf.ends_with(b"\r\n") {
                    return Ok(Some(buf));
                }
                if buf.len() > 4096 {
                    return Err(io::Error::other("unexpected flood of data from device"));
                }
                continue;
            }
            if n == 0 {
                return Err(io::Error::other("connection closed by device"));
            }
            let err = io::Error::last_os_error();
            match err.raw_os_error() {
                // The socket timeout expired: nothing to read yet, try again until deadline.
                // EAGAIN and EWOULDBLOCK are the same value on Linux.
                Some(libc::EAGAIN) | Some(libc::EINTR) => continue,
                _ => return Err(err),
            }
        }
        Ok(None)
    }

    fn write_all(&self, mut data: &[u8]) -> io::Result<()> {
        while !data.is_empty() {
            let n = unsafe {
                libc::write(
                    self.fd.as_raw_fd(),
                    data.as_ptr().cast::<libc::c_void>(),
                    data.len(),
                )
            };
            if n > 0 {
                data = &data[n as usize..];
                continue;
            }
            let err = io::Error::last_os_error();
            if matches!(err.raw_os_error(), Some(libc::EINTR)) {
                continue;
            }
            return Err(err);
        }
        Ok(())
    }
}

/// Connects with a timeout, which a blocking RFCOMM connect does not offer: a device that
/// is out of range would otherwise hang for the stack's own (much longer) timeout.
fn connect(address: [u8; 6], channel: u8, timeout: Duration) -> io::Result<OwnedFd> {
    let raw = unsafe {
        libc::socket(
            AF_BLUETOOTH,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            BTPROTO_RFCOMM,
        )
    };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };

    let addr = SockAddrRc {
        rc_family: AF_BLUETOOTH as libc::sa_family_t,
        rc_bdaddr: address,
        rc_channel: channel,
    };
    let rc = unsafe {
        libc::connect(
            fd.as_raw_fd(),
            (&raw const addr).cast::<libc::sockaddr>(),
            mem::size_of::<SockAddrRc>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        let err = io::Error::last_os_error();
        if !matches!(err.raw_os_error(), Some(libc::EINPROGRESS)) {
            return Err(err);
        }
        wait_writable(&fd, timeout)?;
        let mut sockerr: libc::c_int = 0;
        let mut len = mem::size_of::<libc::c_int>() as libc::socklen_t;
        let got = unsafe {
            libc::getsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                (&raw mut sockerr).cast::<libc::c_void>(),
                &raw mut len,
            )
        };
        if got < 0 {
            return Err(io::Error::last_os_error());
        }
        if sockerr != 0 {
            return Err(io::Error::from_raw_os_error(sockerr));
        }
    }

    // Back to blocking; reads are bounded by SO_RCVTIMEO instead.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags & !libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}

fn wait_writable(fd: &OwnedFd, timeout: Duration) -> io::Result<()> {
    let mut pfd = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLOUT,
        revents: 0,
    };
    let millis = timeout.as_millis().min(i32::MAX as u128) as libc::c_int;
    loop {
        let rc = unsafe { libc::poll(&raw mut pfd, 1, millis) };
        if rc > 0 {
            return Ok(());
        }
        if rc == 0 {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "connect timed out"));
        }
        let err = io::Error::last_os_error();
        if !matches!(err.raw_os_error(), Some(libc::EINTR)) {
            return Err(err);
        }
    }
}

fn set_recv_timeout(fd: &OwnedFd, timeout: Duration) -> io::Result<()> {
    let tv = libc::timeval {
        tv_sec: timeout.as_secs() as libc::time_t,
        tv_usec: timeout.subsec_micros() as libc::suseconds_t,
    };
    let rc = unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            (&raw const tv).cast::<libc::c_void>(),
            mem::size_of::<libc::timeval>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_address;

    #[test]
    fn parses_address_little_endian() {
        assert_eq!(
            parse_address("00:07:80:C4:46:09"),
            Some([0x09, 0x46, 0xC4, 0x80, 0x07, 0x00])
        );
    }

    #[test]
    fn rejects_malformed_addresses() {
        for bad in [
            "",
            "00:07:80:C4:46",
            "00:07:80:C4:46:09:11",
            "0:7:80:C4:46:09",
            "zz:07:80:C4:46:09",
        ] {
            assert_eq!(parse_address(bad), None, "{bad} should not parse");
        }
    }
}
