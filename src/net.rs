//! Minimal HTTPS GET client built on WinHTTP.
//!
//! Using the operating system's HTTP stack keeps the executable small (no TLS
//! library is linked in) and inherits the user's proxy configuration.

use std::ffi::c_void;

type Handle = *mut c_void;

#[link(name = "winhttp")]
extern "system" {
    fn WinHttpOpen(
        agent: *const u16,
        access_type: u32,
        proxy: *const u16,
        bypass: *const u16,
        flags: u32,
    ) -> Handle;
    fn WinHttpConnect(session: Handle, host: *const u16, port: u16, reserved: u32) -> Handle;
    fn WinHttpOpenRequest(
        connect: Handle,
        verb: *const u16,
        path: *const u16,
        version: *const u16,
        referrer: *const u16,
        accept: *const u16,
        flags: u32,
    ) -> Handle;
    fn WinHttpSendRequest(
        request: Handle,
        headers: *const u16,
        headers_len: u32,
        optional: *mut c_void,
        optional_len: u32,
        total_len: u32,
        context: usize,
    ) -> i32;
    fn WinHttpReceiveResponse(request: Handle, reserved: *mut c_void) -> i32;
    fn WinHttpQueryHeaders(
        request: Handle,
        info_level: u32,
        name: *const u16,
        buffer: *mut c_void,
        len: *mut u32,
        index: *mut u32,
    ) -> i32;
    fn WinHttpQueryDataAvailable(request: Handle, len: *mut u32) -> i32;
    fn WinHttpReadData(request: Handle, buffer: *mut c_void, len: u32, read: *mut u32) -> i32;
    fn WinHttpSetTimeouts(session: Handle, resolve: i32, connect: i32, send: i32, receive: i32) -> i32;
    fn WinHttpCloseHandle(handle: Handle) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetLastError() -> u32;
}

const ACCESS_TYPE_DEFAULT_PROXY: u32 = 0;
const FLAG_SECURE: u32 = 0x0080_0000;
const QUERY_STATUS_CODE: u32 = 19;
const QUERY_STATUS_CODE_NUMBER: u32 = 0x2000_0000;
const QUERY_RAW_HEADERS: u32 = 22;
const MAX_BODY: usize = 6 * 1024 * 1024;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct Session(Handle);
impl Drop for Session {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}
struct Connect(Handle);
impl Drop for Connect {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}
struct Request(Handle);
impl Drop for Request {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn parse_url(url: &str) -> Result<(String, String), String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| format!("Unsupported URL: {url}"))?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if host.is_empty() {
        return Err("Empty host".to_string());
    }
    Ok((host.to_string(), path.to_string()))
}

fn read_status(request: Handle) -> u32 {
    let mut code: u32 = 0;
    let mut len = std::mem::size_of::<u32>() as u32;
    let mut index: u32 = 0;
    let ok = unsafe {
        WinHttpQueryHeaders(
            request,
            QUERY_STATUS_CODE | QUERY_STATUS_CODE_NUMBER,
            std::ptr::null(),
            &mut code as *mut u32 as *mut c_void,
            &mut len,
            &mut index,
        )
    };
    if ok == 0 {
        0
    } else {
        code
    }
}

fn header_value(request: Handle, name: &str) -> Option<String> {
    let mut buffer = vec![0u16; 4096];
    let mut len = (buffer.len() * 2) as u32;
    let mut index: u32 = 0;
    let name_w = wide(name);
    let ok = unsafe {
        WinHttpQueryHeaders(
            request,
            QUERY_RAW_HEADERS,
            name_w.as_ptr(),
            buffer.as_mut_ptr() as *mut c_void,
            &mut len,
            &mut index,
        )
    };
    if ok == 0 {
        return None;
    }
    let chars = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buffer[..chars]).trim().to_string())
}

/// Blocking GET. Returns the response body for 2xx statuses.
pub fn http_get(url: &str) -> Result<Vec<u8>, String> {
    http_get_max(url, MAX_BODY)
}

/// Blocking GET for larger payloads (traffic feeds, installers).
pub fn http_get_max(url: &str, max: usize) -> Result<Vec<u8>, String> {
    let mut current = url.to_string();
    for _ in 0..4 {
        let (host, path) = parse_url(&current)?;
        let (body, status, location) = request_once(&host, &path, max)?;
        if (300..400).contains(&status) {
            if let Some(next) = location {
                current = next;
                continue;
            }
        }
        if status == 0 {
            return Err("Network request failed (no response)".to_string());
        }
        if !(200..300).contains(&status) {
            return Err(format!("HTTP {status} from {host}"));
        }
        return Ok(body);
    }
    Err("Too many redirects".to_string())
}

fn request_once(
    host: &str,
    path: &str,
    max: usize,
) -> Result<(Vec<u8>, u32, Option<String>), String> {
    // Planespotters asks for a descriptive agent with contact information.
    let agent = wide("MSFSFlightTracker/1.0 (+https://github.com/msfs-flight-tracker)");
    let session = Session(unsafe {
        WinHttpOpen(
            agent.as_ptr(),
            ACCESS_TYPE_DEFAULT_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        )
    });
    if session.0.is_null() {
        return Err(format!("WinHttpOpen failed (error {})", unsafe { GetLastError() }));
    }
    unsafe { WinHttpSetTimeouts(session.0, 8000, 8000, 10000, 20000) };

    let host_w = wide(host);
    let connect = Connect(unsafe { WinHttpConnect(session.0, host_w.as_ptr(), 443, 0) });
    if connect.0.is_null() {
        return Err(format!("Cannot connect to {host}"));
    }

    let verb = wide("GET");
    let path_w = wide(path);
    let headers = wide("Accept: application/json, image/*\r\n");
    let request = Request(unsafe {
        WinHttpOpenRequest(
            connect.0,
            verb.as_ptr(),
            path_w.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            FLAG_SECURE,
        )
    });
    if request.0.is_null() {
        return Err("WinHttpOpenRequest failed".to_string());
    }

    let sent = unsafe {
        WinHttpSendRequest(
            request.0,
            headers.as_ptr(),
            // WinHTTP wants the header length in characters, not bytes.
            headers.len() as u32 - 1,
            std::ptr::null_mut(),
            0,
            0,
            0,
        )
    };
    if sent == 0 {
        return Err(format!("Request failed (error {})", unsafe { GetLastError() }));
    }
    if unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) } == 0 {
        return Err(format!("No response (error {})", unsafe { GetLastError() }));
    }

    let status = read_status(request.0);
    let location = if (300..400).contains(&status) {
        header_value(request.0, "Location")
    } else {
        None
    };

    let mut body: Vec<u8> = Vec::new();
    loop {
        let mut available: u32 = 0;
        if unsafe { WinHttpQueryDataAvailable(request.0, &mut available) } == 0 {
            break;
        }
        if available == 0 {
            break;
        }
        let mut chunk = vec![0u8; available as usize];
        let mut read: u32 = 0;
        if unsafe {
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr() as *mut c_void,
                available,
                &mut read,
            )
        } == 0
        {
            break;
        }
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read as usize]);
        if body.len() > max {
            return Err("Response too large".to_string());
        }
    }

    Ok((body, status, location))
}

/// Percent-encodes a query component (spaces, quotes, non-ASCII).
pub fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub fn get_json(url: &str) -> Result<serde_json::Value, String> {
    let body = http_get(url)?;
    serde_json::from_slice(&body).map_err(|e| format!("Malformed response: {e}"))
}
